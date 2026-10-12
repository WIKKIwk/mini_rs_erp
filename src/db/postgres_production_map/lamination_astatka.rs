use super::*;
use crate::core::production_map::{
    LaminatsiyaAstatkaCommitResult, LayerRemainderInput, OrderRunStatus,
};
use crate::core::quantity::{erp_quantity_to_units, positive_erp_quantity};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn db_error(error: sqlx::Error) -> ProductionMapError {
    tracing::error!(%error, "atomic lamination astatka storage failed");
    ProductionMapError::StoreFailed
}

impl PostgresProductionMapStore {
    pub(super) async fn laminatsiya_astatka_commit_by_request_id(
        &self,
        actor_ref: &str,
        request_id: &str,
    ) -> Result<Option<(Value, LaminatsiyaAstatkaCommitResult)>, ProductionMapError> {
        let row: Option<(Value, Value)> = sqlx::query_as(
            "SELECT request_json,response_json FROM mini_lamination_astatka_operations
             WHERE recorded_by_ref=$1 AND request_id=$2",
        )
        .bind(actor_ref)
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        row.map(|(request, response)| {
            serde_json::from_value(response)
                .map(|response| (request, response))
                .map_err(|_| ProductionMapError::StoreFailed)
        })
        .transpose()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn commit_laminatsiya_astatka(
        &self,
        request_id: String,
        request_json: Value,
        report: Option<LaminatsiyaAstatkaReport>,
        expected_session: OrderRunSession,
        actor: QueueActionActor,
        queue_write: Option<QueueActionProgressWrite>,
        mut remainders: Vec<LayerRemainderInput>,
    ) -> Result<LaminatsiyaAstatkaCommitResult, ProductionMapError> {
        if !(8..=128).contains(&request_id.len())
            || actor.ref_.trim().is_empty()
            || report.as_ref().is_some_and(|r| {
                expected_session.order_id != r.order_id || expected_session.apparatus != r.apparatus
            })
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let order_id = expected_session.order_id.clone();
        let apparatus = expected_session.apparatus.clone();
        let action = match request_json.get("action") {
            None => crate::core::production_map::queue_state::ApparatusQueueAction::Pause,
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|_| ProductionMapError::ProgressInputInvalid)?,
        };
        let completing =
            action == crate::core::production_map::queue_state::ApparatusQueueAction::Complete;
        if (!completing
            && action != crate::core::production_map::queue_state::ApparatusQueueAction::Pause)
            || completing == report.is_some()
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        transaction_locks::lock_order_and_apparatuses_tx(&mut tx, &order_id, &[&apparatus]).await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(format!("lamination-astatka:{}:{}", actor.ref_, request_id))
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let replay: Option<(Value, Value)> = sqlx::query_as(
            "SELECT request_json,response_json FROM mini_lamination_astatka_operations
             WHERE recorded_by_ref=$1 AND request_id=$2",
        )
        .bind(&actor.ref_)
        .bind(&request_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
        if let Some((old, response)) = replay {
            if old != request_json {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            return serde_json::from_value(response).map_err(|_| ProductionMapError::StoreFailed);
        }
        let frozen: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_order_control_states WHERE order_id=$1
             AND state IN ('frozen','freeze_requested'))",
        )
        .bind(&order_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        if frozen {
            return Err(ProductionMapError::OrderFrozen);
        }
        sqlx::query("SELECT session_id FROM mini_order_run_sessions WHERE order_id=$1 FOR UPDATE")
            .bind(&order_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(db_error)?;
        let sessions = stage_execution::sessions_tx(&mut tx, &order_id).await?;
        let current = sessions
            .iter()
            .filter(|s| s.apparatus == apparatus)
            .max_by(|a, b| {
                (a.started_at_unix, &a.session_id).cmp(&(b.started_at_unix, &b.session_id))
            })
            .ok_or(ProductionMapError::OrderNotStarted)?;
        if current != &expected_session
            || !matches!(
                current.status,
                OrderRunStatus::Active
                    | OrderRunStatus::Paused
                    | OrderRunStatus::RollDetached
                    | OrderRunStatus::Completed
            )
            || current
                .payload_json
                .get("lamination_astatka_shift_closed")
                .and_then(Value::as_bool)
                == Some(true)
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        if current
            .payload_json
            .get("input_wip_source_kind")
            .and_then(Value::as_str)
            == Some("opening_wip")
            || crate::core::production_map::order_run_input_links_from_payload(
                &current.payload_json,
            )
            .map_err(|_| ProductionMapError::StoreFailed)?
            .iter()
            .any(|link| {
                link.source_kind == crate::core::production_map::OrderRunInputSourceKind::OpeningWip
                    && link.status == crate::core::production_map::OrderRunInputStatus::InUse
            })
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        // Validate the complete current physical source set while holding the
        // same order/apparatus/source locks as every stock claim or return.
        let sources = layer_remainder::session_sources_tx(&mut tx, current).await?;
        let available: BTreeSet<_> = sources
            .iter()
            .map(|s| (s.source_kind.as_str(), s.source_id.as_str()))
            .collect();
        let submitted: BTreeSet<_> = remainders
            .iter()
            .map(|s| (s.source_kind.as_str(), s.source_id.as_str()))
            .collect();
        if submitted.len() != remainders.len() || submitted != available || submitted.is_empty() {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        for input in &remainders {
            let source = sources
                .iter()
                .find(|s| s.source_kind == input.source_kind && s.source_id == input.source_id)
                .ok_or(ProductionMapError::ProgressInputInvalid)?;
            if input.apparatus != apparatus
                || input.order_id != order_id
                || input.session_id != current.session_id
                || input.expected_revision != source.revision
                || !input.remaining_net_kg.is_finite()
                || input.remaining_net_kg < 0.0
                || (input.remaining_net_kg > 0.0
                    && positive_erp_quantity(input.remaining_net_kg).is_none())
                || (input.remaining_net_kg > 0.0
                    && (positive_erp_quantity(source.source_net_kg).is_none()
                        || positive_erp_quantity(source.source_meter).is_none()
                        || erp_quantity_to_units(input.remaining_net_kg)
                            > erp_quantity_to_units(source.source_net_kg)))
            {
                return Err(ProductionMapError::ProgressBatchCorrectionConflict);
            }
        }
        let output_batches = if let Some(write) = &queue_write {
            if (!completing && expected_session.status != OrderRunStatus::Active)
                || (completing && expected_session.status == OrderRunStatus::Completed)
                || write.event.order_id != order_id
                || write.apparatus != apparatus
                || write.event.action != action
                || write.session.as_ref().is_none_or(|s| {
                    s.session_id != expected_session.session_id
                        || s.status
                            != if completing {
                                OrderRunStatus::Completed
                            } else {
                                OrderRunStatus::Paused
                            }
                })
            {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            apply_queue_progress_tx(&mut tx, write).await?;
            if write.progress_batches.is_empty() {
                write.progress_batch.clone().into_iter().collect()
            } else {
                write.progress_batches.clone()
            }
        } else {
            if completing || expected_session.status == OrderRunStatus::Active {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            Vec::new()
        };
        // Reload the committed queue session: Complete may have stamped its
        // work report inside apply_queue_progress_tx. Keep that exact stamp.
        let mut session = stage_execution::sessions_tx(&mut tx, &order_id)
            .await?
            .into_iter()
            .find(|s| s.session_id == expected_session.session_id)
            .ok_or(ProductionMapError::OrderNotStarted)?;
        if let Some(report) = &report {
            astatka_helpers::put_laminatsiya_astatka_report_tx(&mut tx, report).await?;
        }
        let report_id = report
            .as_ref()
            .map(|r| r.report_id.clone())
            .or_else(|| queue_write.as_ref().map(|w| w.event.event_id.clone()))
            .ok_or(ProductionMapError::ProgressInputInvalid)?;
        // Deterministic source lock order and compact operation keys avoid both
        // concurrent-return deadlocks and the per-layer request length limit.
        remainders
            .sort_by(|a, b| (&a.source_kind, &a.source_id).cmp(&(&b.source_kind, &b.source_id)));
        let operation_id = format!("lamination-astatka:{:032x}", rand::random::<u128>());
        let mut saved = Vec::with_capacity(remainders.len());
        let raw_released = remainders.iter().any(|r| r.source_kind == "raw_material");
        for (index, mut input) in remainders.into_iter().enumerate() {
            // The original user revision was checked before applying the exact
            // prepared queue action. It may itself touch this same
            // locked roll; only that known write may advance the witness.
            if input.source_kind == "progress_batch"
                && queue_write.as_ref().is_some_and(|write| {
                    write
                        .progress_batch_updates
                        .iter()
                        .any(|batch| batch.batch_id == input.source_id)
                })
            {
                input.expected_revision =
                    order_query_helpers::load_progress_batch(&mut *tx, &input.source_id)
                        .await?
                        .ok_or(ProductionMapError::ProgressBatchNotFound)?
                        .revision;
            }
            input.request_id = format!("{operation_id}:{index}");
            saved.push(
                layer_remainder::record_layer_remainder_tx(
                    &mut tx,
                    input,
                    actor.clone(),
                    &mut session,
                    true,
                )
                .await?,
            );
        }
        session.payload_json["lamination_astatka_shift_closed"] = json!(true);
        session.payload_json["lamination_astatka_report_id"] = json!(report_id);
        if completing {
            session.payload_json["lamination_layer_return_action"] = json!("complete");
        }
        session.payload_json["lamination_astatka_layers"] = json!(saved.iter().map(|r| json!({
            "source_kind":r.source_kind,"source_id":r.source_id,"remaining_net_kg":r.remaining_net_kg,
            "operation_id":r.operation_id,"qr_payload":r.qr_payload,
        })).collect::<Vec<_>>());
        // Keep the exact previous resources in immutable operation/session
        // audit; a later Resume must scan fresh stocks, never reuse consumed IDs.
        if raw_released {
            session.payload_json["lamination_astatka_raw_released"] = json!(true);
            if let Some(mut resources) = ProductionQrSessionResources::recorded(&session) {
                session.payload_json["lamination_astatka_released_raw_materials"] =
                    json!(resources.raw_materials);
                resources.raw_materials.clear();
                resources.write_to_session(&mut session);
            }
        }
        session.updated_at_unix = crate::core::production_map::stage_execution::report_now();
        progress_helpers::put_order_run_session_tx(&mut tx, &session).await?;
        if !completing && session.status == OrderRunStatus::Completed {
            stage_execution::stamp_report_tx(&mut tx, &mut session, &report_id, &actor).await?;
            progress_helpers::put_order_run_session_tx(&mut tx, &session).await?;
        }
        lifecycle::refresh_production_order_lifecycle_tx(
            &mut tx,
            &order_id,
            &actor,
            &operation_id,
            if completing {
                "lamination_complete_layer_remainders"
            } else {
                "lamination_astatka"
            },
        )
        .await?;
        let result = LaminatsiyaAstatkaCommitResult {
            action,
            request_id: request_id.clone(),
            report,
            session,
            progress_batches: output_batches,
            remainders: saved,
        };
        sqlx::query("INSERT INTO mini_lamination_astatka_operations
            (operation_id,apparatus,order_id,session_id,recorded_by_ref,request_id,report_id,request_json,response_json)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(operation_id).bind(&apparatus).bind(&order_id)
            .bind(&result.session.session_id).bind(&actor.ref_).bind(request_id)
            .bind(&report_id).bind(request_json).bind(json!(result))
            .execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(result)
    }
}
