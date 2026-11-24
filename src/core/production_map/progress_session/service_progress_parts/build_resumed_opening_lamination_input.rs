impl ProductionMapService {
    /// A returned opening roll retains its passport while becoming the next
    /// physical input of the released lamination session.
    async fn build_resumed_opening_lamination_input(
        &self,
        context: ProgressBuildContext<'_>,
        record: OpeningWipBatchRecord,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let ProgressBuildContext {
            apparatus,
            order_id,
            order_map,
            action,
            actor,
            canonical,
            now,
        } = context;
        let current = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
        let shift_closed = current
            .payload_json
            .get("lamination_astatka_shift_closed")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        if !apparatus::uses_lamination_workflow(canonical)
            || !lamination_input_was_released(&current)
            || record.intake.status != OpeningWipIntakeStatus::Confirmed
            || record.intake.order_id != order_id
            || record.batch.order_id != order_id
            || record.batch.wip_status != OpeningWipBatchStatus::Waiting
            || (!shift_closed
                && (record.batch.parent_batch_id.is_empty()
                    || current.payload_json["layer_remainder_detached_input_batch_id"].as_str()
                        != Some(record.batch.parent_batch_id.as_str())))
        {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
        let stage = Self::opening_wip_target_stage(
            order_map,
            &record.intake,
            apparatus,
            &current.stage_node_id,
        )
        .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
        let mut links = progress_links_from_opening_wip(&record, &stage.node_id);
        links.stage_node_id = current.stage_node_id.clone();
        let mut payload = resumed_handoff_session_payload(&current, &links);
        let mut inputs = order_run_input_links_from_payload(&payload)
            .map_err(|_| ProductionMapError::ProgressBatchNotResumable)?;
        if inputs.iter().any(|link| {
            link.status == OrderRunInputStatus::InUse
                || (link.source_kind == OrderRunInputSourceKind::OpeningWip
                    && link.input_batch_id == record.batch.batch_id)
        }) {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
        let sequence_no = inputs
            .iter()
            .map(|link| link.sequence_no)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ProductionMapError::ProgressInputInvalid)?;
        inputs.push(OrderRunInputLink {
            input_batch_id: record.batch.batch_id.clone(),
            input_qr_payload: record.batch.qr_payload.clone(),
            source_apparatus: links.apparatus.clone(),
            source_kind: OrderRunInputSourceKind::OpeningWip,
            stage_node_id: current.stage_node_id.clone(),
            sequence_no,
            status: OrderRunInputStatus::InUse,
            linked_at_unix: now,
            processed_at_unix: None,
        });
        write_order_run_input_links(&mut payload, &inputs);
        if let Some(object) = payload.as_object_mut() {
            for field in [
                "layer_remainder_detached_input_batch_id",
                "lamination_astatka_shift_closed",
                "lamination_astatka_raw_released",
            ] {
                object.remove(field);
            }
        }
        let session = OrderRunSession {
            status: OrderRunStatus::Active,
            worker_role: actor.role.trim().into(),
            worker_ref: actor.ref_.trim().into(),
            worker_display_name: actor.display_name.trim().into(),
            updated_at_unix: now,
            payload_json: payload,
            ..current
        };
        let mut event = zero_quantity_event(
            ProgressRecordContext {
                session: &session,
                apparatus,
                order_id,
                action,
                actor,
            },
            record.batch.batch_id.clone(),
            record.batch.qr_payload.clone(),
            resume_event_payload(),
        );
        event.payload_json["opening_wip_input_expected_record"] =
            serde_json::to_value(&record).map_err(|_| ProductionMapError::StoreFailed)?;
        event.payload_json["wip_input_map_fingerprint"] =
            serde_json::json!(super::wip_route::production_map_fingerprint(order_map));
        let input = opening_wip_batch_in_use(record.batch, apparatus, &session.session_id, now);
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates: Vec::new(),
            opening_wip_batch_updates: vec![input],
        })
    }

    /// Claim the next physical input after a lamination shift returned or
    /// exhausted its mounted roll. This preserves the upstream producer.
    async fn build_resumed_lamination_input(
        &self,
        context: ProgressBuildContext<'_>,
        mut batch: OrderProgressBatch,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let ProgressBuildContext {
            apparatus, order_id, order_map, action, actor, canonical, now,
        } = context;
        if !apparatus::uses_lamination_workflow(canonical)
            || batch.order_id.trim() != order_id
            || !super::wip_route::actionable_wip(&batch)
        {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
        let current = self.store.active_order_run_session(apparatus, order_id).await?
            .filter(|session| matches!(session.status, OrderRunStatus::Paused | OrderRunStatus::RollDetached))
            .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
        let shift_closed = current.payload_json.get("lamination_astatka_shift_closed")
            .and_then(serde_json::Value::as_bool) == Some(true);
        if !lamination_input_was_released(&current)
            || (!shift_closed && (!batch.is_layer_remainder()
                || current.payload_json.get("layer_remainder_detached_input_batch_id")
                    .and_then(serde_json::Value::as_str) != Some(batch.parent_batch_id.as_str())))
        {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
        let mut route = super::wip_route::resolve_wip_input_route(order_map, &batch)?;
        if !route.consumer_apparatus_ids.iter().any(|id| id == apparatus)
            || !chain::stage_node_ids_match_for_map(order_map, &route.stage_node_id, &current.stage_node_id)
        {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
        let expected = self.store.progress_batch(&batch.batch_id).await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        if super::wip_route::normalized_route_batch(&expected) != batch {
            return Err(ProductionMapError::WipRouteChanged);
        }
        let mut links = progress_links_from_batch(&batch);
        links.stage_node_id = current.stage_node_id.clone();
        let mut payload = resumed_handoff_session_payload(&current, &links);
        let mut inputs = order_run_input_links_from_payload(&payload)
            .map_err(|_| ProductionMapError::ProgressBatchNotResumable)?;
        if inputs.iter().any(|link| link.status == OrderRunInputStatus::InUse
            || link.input_batch_id == batch.batch_id)
        {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
        let sequence_no = inputs.iter().map(|link| link.sequence_no).max().unwrap_or(0)
            .checked_add(1).ok_or(ProductionMapError::ProgressInputInvalid)?;
        inputs.push(OrderRunInputLink {
            input_batch_id: batch.batch_id.clone(), input_qr_payload: batch.qr_payload.clone(),
            source_apparatus: batch.apparatus.clone(), source_kind: OrderRunInputSourceKind::ProgressBatch,
            stage_node_id: current.stage_node_id.clone(), sequence_no,
            status: OrderRunInputStatus::InUse, linked_at_unix: now, processed_at_unix: None,
        });
        write_order_run_input_links(&mut payload, &inputs);
        if let Some(object) = payload.as_object_mut() {
            object.remove("layer_remainder_detached_input_batch_id");
            object.remove("lamination_astatka_shift_closed");
            object.remove("lamination_astatka_raw_released");
        }
        let session = OrderRunSession {
            status: OrderRunStatus::Active, worker_role: actor.role.trim().into(),
            worker_ref: actor.ref_.trim().into(), worker_display_name: actor.display_name.trim().into(),
            updated_at_unix: now, payload_json: payload, ..current
        };
        route.stage_node_id = session.stage_node_id.clone();
        batch.payload_json["wip_route_binding"] = serde_json::to_value(route)
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut event = zero_quantity_event(
            ProgressRecordContext { session: &session, apparatus, order_id, action, actor },
            batch.batch_id.clone(), batch.qr_payload.clone(), resume_event_payload(),
        );
        event.payload_json["wip_input_expected_batch"] = serde_json::to_value(expected)
            .map_err(|_| ProductionMapError::StoreFailed)?;
        event.payload_json["wip_input_map_fingerprint"] = serde_json::json!(
            super::wip_route::production_map_fingerprint(order_map));
        let input = wip_batch_in_use(batch, apparatus, &session.session_id, now);
        Ok(QueueProgressRecords {
            session: Some(session), progress_event: Some(event), progress_batch: None,
            progress_batches: Vec::new(), progress_batch_updates: vec![input],
            opening_wip_batch_updates: Vec::new(),
        })
    }

    async fn build_resumed_work_session(
        &self,
        context: ProgressBuildContext<'_>,
        mut session: OrderRunSession,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let ProgressBuildContext {
            apparatus, order_id, order_map, action, actor, now, ..
        } = context;
        let links = session_progress_links(&session);
        let stage = chain::work_stage_for_station(order_map, apparatus, &links.stage_node_id)
            .ok_or(ProductionMapError::ProgressBatchNotResumable)?;

        // Resume the producer's work, not an output roll already handed to the
        // next stage. Only the session's input must still belong to this work.
        if links.source_kind == "opening_wip" {
            let record = self.store
                .opening_wip_batch(&links.batch_id, &links.qr_payload)
                .await?
                .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
            if record.intake.status != OpeningWipIntakeStatus::Confirmed
                || record.intake.order_id.trim() != order_id
                || record.batch.order_id.trim() != order_id
                || record.batch.wip_status != OpeningWipBatchStatus::InUse
                || record.batch.used_by_session_id.trim() != session.session_id.trim()
                || !super::types::apparatus_ids_match(&record.batch.used_by_apparatus, apparatus)
                || Self::opening_wip_target_stage(
                    order_map, &record.intake, apparatus, &stage.node_id,
                ).is_none()
            {
                return Err(ProductionMapError::ProgressBatchNotResumable);
            }
        } else if !links.batch_id.trim().is_empty() {
            let input = self
                .previous_stage_active_progress_batch(
                    order_id,
                    order_map,
                    apparatus,
                    &QueueProgressInput {
                        progress_batch_id: links.batch_id.clone(),
                        qr_payload: links.qr_payload.clone(),
                        ..QueueProgressInput::default()
                    },
                    &session.session_id,
                    &stage.node_id,
                    None,
                )
                .await?
                .ok_or(ProductionMapError::ProgressBatchNotResumable)?;
            if input.wip_status != OrderProgressBatchWipStatus::InUse
                || input.used_by_session_id.trim() != session.session_id.trim()
                || !super::types::apparatus_ids_match(&input.used_by_apparatus, apparatus)
            {
                return Err(ProductionMapError::ProgressBatchNotResumable);
            }
        } else if chain::previous_work_stage_for_node(order_map, &stage.node_id).is_some() {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }

        if super::temporary_pause::is_temporarily_paused(&session) {
            let expected_session = serde_json::to_value(&session).map_err(|_| ProductionMapError::StoreFailed)?;
            let inputs = super::temporary_pause::held_inputs(self, &session).await?;
            let mut payload = super::temporary_pause::resume(&mut session, actor, now)?;
            payload[super::temporary_pause::EXPECTED_SESSION] = expected_session;
            payload["temporary_pause_held_inputs"] = inputs;
            payload["temporary_pause_map_fingerprint"] = serde_json::json!(super::wip_route::production_map_fingerprint(order_map));
            let event = zero_quantity_event(
                ProgressRecordContext { session: &session, apparatus, order_id, action, actor },
                String::new(), String::new(), payload);
            return Ok(QueueProgressRecords {
                session: Some(session), progress_event: Some(event), progress_batch: None,
                progress_batches: Vec::new(), progress_batch_updates: Vec::new(),
                opening_wip_batch_updates: Vec::new(),
            });
        }
        // A normal output pause must have a real output in this session. Its
        // downstream WIP status and the number of earlier outputs are irrelevant.
        let has_output = self.store.progress_batches_for_order(order_id).await?
            .iter()
            .any(|batch| {
                batch.session_id == session.session_id
                    && batch.action.creates_resumable_output()
                    && batch.status.is_resumable()
                    && super::types::apparatus_ids_match(&batch.apparatus, apparatus)
            });
        let reopened_work = session.worker_role == actor.role
            && session.worker_ref == actor.ref_
            && session.payload_json.get("reactivation_token").and_then(serde_json::Value::as_str).is_some_and(|token| token.len() == 64)
            && session.payload_json.get("_reactivation_of_session_id").and_then(serde_json::Value::as_str).is_some_and(|id| !id.is_empty())
            && (crate::core::production_map::reactivation::reclose_control(&session).is_some()
                || session.payload_json.get("_reactivation_started").and_then(serde_json::Value::as_bool) == Some(true));
        if !has_output && !reopened_work && !session.is_print_transfer_continuation() {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
        session.payload_json = resumed_handoff_session_payload(&session, &links);
        // Ordinary resume begins a new output cycle, including legacy sessions
        // and a completed operation reopened for further production.
        session.payload_json["wip_waiting_for_resume"] = serde_json::json!(true);
        session.payload_json["resumed_without_progress_qr"] = serde_json::json!(true);
        if let Some(payload) = session.payload_json.as_object_mut() {
            payload.remove("lamination_astatka_shift_closed");
            payload.remove("lamination_astatka_raw_released");
        }
        session.status = OrderRunStatus::Active;
        session.worker_role = actor.role.trim().to_string();
        session.worker_ref = actor.ref_.trim().to_string();
        session.worker_display_name = actor.display_name.trim().to_string();
        session.updated_at_unix = now;
        let event = zero_quantity_event(
            ProgressRecordContext { session: &session, apparatus, order_id, action, actor },
            links.batch_id,
            links.qr_payload,
            resume_event_payload(),
        );
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates: Vec::new(),
            opening_wip_batch_updates: Vec::new(),
        })
    }

    async fn build_frozen_progress(
        &self,
        apparatus: &str,
        order_id: &str,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        now: i64,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let description = progress.description.trim().to_string();
        let mut session = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        ensure_wip_started_at(&mut session);
        let input_progress = session_progress_links(&session);
        let metrics = ProgressMetrics::default();
        let mut session_payload = preserve_qolip_lineage(
            &session,
            progress_session_payload(
                queue_state::ApparatusQueueAction::Freeze,
                0.0,
                &non_empty_or(&progress.uom, "kg"),
                metrics,
                &description,
                &input_progress,
            ),
        );
        session_payload["frozen_order"] = serde_json::json!(true);
        if progress.freeze_with_issue {
            session_payload["freeze_with_issue"] = serde_json::json!(true);
            session_payload["issue_note"] = serde_json::json!(&description);
        }
        let session = OrderRunSession {
            status: OrderRunStatus::Frozen,
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            updated_at_unix: now,
            payload_json: session_payload,
            ..session
        };
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action: queue_state::ApparatusQueueAction::Freeze,
            actor,
        };
        let mut progress_event = zero_quantity_event(
            context,
            String::new(),
            String::new(),
            progress_event_payload(
                queue_state::ApparatusQueueAction::Freeze,
                metrics,
                &description,
            ),
        );
        progress_event.description = description;
        progress_event.payload_json["frozen_order"] = serde_json::json!(true);
        if progress.freeze_with_issue {
            progress_event.payload_json["freeze_with_issue"] = serde_json::json!(true);
            let issue_note = progress_event.description.clone();
            progress_event.payload_json["issue_note"] = serde_json::json!(issue_note);
        }
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(progress_event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates: Vec::new(),
            opening_wip_batch_updates: Vec::new(),
        })
    }
}
