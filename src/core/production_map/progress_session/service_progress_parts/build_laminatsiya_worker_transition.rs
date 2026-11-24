impl ProductionMapService {
    #[allow(clippy::too_many_arguments)]
    async fn build_laminatsiya_worker_transition(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        now: i64,
        canonical: &RuntimeApparatusConfiguration,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        if !apparatus::uses_lamination_workflow(canonical)
            || (progress.worker_handoff && progress.remove_roll_from_apparatus)
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let remove_roll = progress.remove_roll_from_apparatus;
        let mut session = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        ensure_wip_started_at(&mut session);
        if (!remove_roll && session.status != OrderRunStatus::Active)
            || (remove_roll && session.status != OrderRunStatus::Paused)
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let input_progress = session_progress_links(&session);
        if input_progress.batch_id.trim().is_empty() {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        if input_progress.source_kind == "opening_wip" {
            return self
                .build_opening_wip_laminatsiya_worker_transition(
                    apparatus,
                    order_id,
                    order_map,
                    action,
                    actor,
                    progress,
                    now,
                    canonical,
                    session,
                    input_progress,
                )
                .await;
        }
        let input_batch = self
            .store
            .progress_batch(&input_progress.batch_id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        let route = super::wip_route::resolve_wip_input_route(order_map, &input_batch)?;
        let stage = chain::work_stage_for_station(order_map, apparatus, &session.stage_node_id)
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let used_by_apparatus = if input_batch.used_by_apparatus.trim().is_empty() {
            input_batch.current_apparatus.as_str()
        } else {
            input_batch.used_by_apparatus.as_str()
        };
        if input_batch.order_id.trim() != order_id.trim()
            || input_batch.wip_status != OrderProgressBatchWipStatus::InUse
            || !super::types::apparatus_ids_match(used_by_apparatus, apparatus)
            || (!input_batch.used_by_session_id.trim().is_empty()
                && input_batch.used_by_session_id.trim() != session.session_id.trim())
            || !route.consumer_apparatus_ids.iter().any(|id| id == apparatus)
            || !chain::stage_node_ids_match_for_map(order_map, &route.stage_node_id, &stage.node_id)
            || (input_batch.payload_json.get("wip_route_binding").is_some()
                && route.stage_node_id != session.stage_node_id)
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        if remove_roll
            && input_batch
                .payload_json
                .get("worker_handoff")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let metrics = if remove_roll {
            validated_laminatsiya_removed_roll_metrics(canonical, &progress)?
        } else {
            validated_laminatsiya_worker_handoff_metrics(canonical, &progress)?
        };
        let description = progress.description.trim().to_string();
        let updated_input_batch = if remove_roll {
            wip_batch_removed_from_apparatus(
                input_batch.clone(),
                apparatus,
                metrics
                    .finished_goods_meter
                    .ok_or(ProductionMapError::ProgressInputInvalid)?,
                metrics
                    .finished_goods_kg
                    .ok_or(ProductionMapError::ProgressInputInvalid)?,
                metrics
                    .bobina_kg
                    .ok_or(ProductionMapError::ProgressInputInvalid)?,
                now,
            )
        } else {
            wip_batch_worker_handoff(input_batch.clone(), apparatus, &session.session_id, now)
        };
        let session_payload = if remove_roll {
            removed_roll_session_payload(metrics, &description, &input_progress)
        } else {
            worker_handoff_session_payload(metrics, &description, &input_progress)
        };
        let session = OrderRunSession {
            status: if remove_roll {
                OrderRunStatus::RollDetached
            } else {
                OrderRunStatus::Paused
            },
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            updated_at_unix: now,
            payload_json: preserve_qolip_lineage(&session, session_payload),
            ..session
        };
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action,
            actor,
        };
        let event = progress_metrics_event(
            context,
            input_batch.batch_id,
            input_batch.qr_payload,
            metrics,
            &description,
            if remove_roll {
                "roll_removed_from_apparatus"
            } else {
                "worker_handoff"
            },
        );
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates: vec![updated_input_batch],
            opening_wip_batch_updates: Vec::new(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_opening_wip_laminatsiya_worker_transition(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        now: i64,
        canonical: &RuntimeApparatusConfiguration,
        session: OrderRunSession,
        input_progress: SessionProgressLinks,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let record = self
            .store
            .opening_wip_batch(&input_progress.batch_id, &input_progress.qr_payload)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        if record.intake.status != OpeningWipIntakeStatus::Confirmed
            || record.intake.order_id.trim() != order_id.trim()
            || record.batch.order_id.trim() != order_id.trim()
            || record.batch.wip_status != OpeningWipBatchStatus::InUse
            || Self::opening_wip_target_stage(
                order_map,
                &record.intake,
                apparatus,
                &input_progress.stage_node_id,
            )
            .is_none()
            || !super::types::apparatus_ids_match(
                &record.batch.used_by_apparatus,
                apparatus,
            )
            || record.batch.used_by_session_id.trim() != session.session_id.trim()
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let remove_roll = progress.remove_roll_from_apparatus;
        if remove_roll
            && session
                .payload_json
                .get("worker_handoff")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let metrics = if remove_roll {
            validated_laminatsiya_removed_roll_metrics(canonical, &progress)?
        } else {
            validated_laminatsiya_worker_handoff_metrics(canonical, &progress)?
        };
        let description = progress.description.trim().to_string();
        let session_payload = if remove_roll {
            removed_roll_session_payload(metrics, &description, &input_progress)
        } else {
            worker_handoff_session_payload(metrics, &description, &input_progress)
        };
        let session = OrderRunSession {
            status: if remove_roll {
                OrderRunStatus::RollDetached
            } else {
                OrderRunStatus::Paused
            },
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            updated_at_unix: now,
            payload_json: preserve_qolip_lineage(&session, session_payload),
            ..session
        };
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action,
            actor,
        };
        let event = progress_metrics_event(
            context,
            record.batch.batch_id.clone(),
            record.batch.qr_payload.clone(),
            metrics,
            &description,
            if remove_roll {
                "roll_removed_from_apparatus"
            } else {
                "worker_handoff"
            },
        );
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates: Vec::new(),
            opening_wip_batch_updates: if remove_roll {
                vec![opening_wip_batch_waiting(record.batch, now)]
            } else {
                Vec::new()
            },
        })
    }
}

enum MergeInputRecord {
    Progress(OrderProgressBatch),
    Opening(OpeningWipBatchRecord),
}

impl MergeInputRecord {
    fn batch_id(&self) -> &str {
        match self {
            Self::Progress(batch) => &batch.batch_id,
            Self::Opening(record) => &record.batch.batch_id,
        }
    }

    fn links(&self, stage_node_id: &str) -> SessionProgressLinks {
        match self {
            Self::Progress(batch) => progress_links_from_batch(batch),
            Self::Opening(record) => progress_links_from_opening_wip(record, stage_node_id),
        }
    }

    fn material_balance_payload(&self, splice_waste_kg: Option<f64>) -> serde_json::Value {
        let (meter, net_kg) = match self {
            Self::Progress(batch) => (
                batch.finished_goods_meter.or_else(|| {
                    batch
                        .uom
                        .trim()
                        .eq_ignore_ascii_case("m")
                        .then_some(batch.produced_qty)
                }),
                batch.finished_goods_kg,
            ),
            Self::Opening(record) => (
                record.batch.finished_goods_meter.or_else(|| {
                    record
                        .batch
                        .uom
                        .trim()
                        .eq_ignore_ascii_case("m")
                        .then_some(record.batch.quantity)
                        .flatten()
                }),
                record.batch.finished_goods_kg,
            ),
        };
        serde_json::json!({
            "processed_input_batch_id": self.batch_id(),
            "processed_input_meter": meter,
            "processed_input_net_kg": net_kg,
            "splice_waste_kg": splice_waste_kg,
            "output_measurement_deferred": true,
            "diameter_combined": false,
        })
    }
}

fn laminatsiya_merge_contained_kadr_count(
    active: Option<usize>,
    scanned: Option<usize>,
) -> Result<Option<usize>, ProductionMapError> {
    match (active, scanned) {
        (Some(active), Some(scanned)) if active != scanned => {
            Err(ProductionMapError::MergeInputFrameCountMismatch {
                active_kadr_count: active,
                scanned_kadr_count: scanned,
            })
        }
        (Some(active), Some(_)) => Ok(Some(active)),
        (None, None) => Ok(None),
        _ => Err(ProductionMapError::MergeInputNotAccepted),
    }
}
