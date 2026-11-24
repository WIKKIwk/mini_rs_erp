impl ProductionMapService {
    async fn queue_action_intent(
        &self,
        apparatus: &str,
        order_id: &str,
        requested_action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: &mut QueueProgressInput,
    ) -> Result<QueueActionIntent, ProductionMapError> {
        // Resolve the occurrence before applying its hold. A scanned WIP can
        // target a later occurrence of the same physical apparatus.
        let initial_session = self.store.active_order_run_session(apparatus, order_id).await?;
        let mut hold_stage = initial_session.as_ref().map(|s| s.stage_node_id.clone()).unwrap_or_default();
        if requested_action == queue_state::ApparatusQueueAction::Start {
            let input = if !progress.progress_batch_id.trim().is_empty() {
                self.store.progress_batch(progress.progress_batch_id.trim()).await?
            } else if !progress.qr_payload.trim().is_empty() {
                self.store.progress_batch_by_qr(progress.qr_payload.trim()).await?
            } else { None };
            if let Some(batch) = input {
                let map = self.store.map_by_id(order_id).await?.ok_or(ProductionMapError::MapNotFound)?;
                if let Ok(route) = self.projection_map(&map).route(&batch) {
                    hold_stage = chain::work_stage_for_station(&map, apparatus, &route.stage_node_id)
                        .map(|stage| stage.node_id).ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
                }
            }
        }
        if hold_stage.is_empty() {
            let map = self.store.map_by_id(order_id).await?.ok_or(ProductionMapError::MapNotFound)?;
            hold_stage = chain::work_stage_for_station(&map, apparatus, "")
                .map(|s| s.node_id.clone()).ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
        }
        let control = self.effective_order_control_for_stage(order_id, apparatus, &hold_stage).await?;
        let resume_pause_allowed = self.store.supports_resume_pause()
            && control.state == OrderControlState::Active && control.early_close.is_none();
        let freeze_request_finalization = control.state == OrderControlState::FreezeRequested
            && (requested_action.creates_resumable_output()
                || requested_action == queue_state::ApparatusQueueAction::Freeze);
        let freeze_request_safe_stop = control.state == OrderControlState::FreezeRequested
            && requested_action.creates_resumable_output();
        let freeze_request_safe_stop_has_output = progress.has_reported_output();
        let freeze_request_safe_stop_with_issue = freeze_request_safe_stop
            && !freeze_request_safe_stop_has_output
            && !progress.description.trim().is_empty();
        let freeze_with_issue = progress.freeze_with_issue
            || (requested_action == queue_state::ApparatusQueueAction::Freeze
                && control.state == OrderControlState::Active);
        let queue_action = if freeze_with_issue || freeze_request_finalization {
            queue_state::ApparatusQueueAction::Freeze
        } else {
            requested_action
        };
        // Queue freeze and WIP output are distinct effects of the same request.
        let progress_action = if freeze_request_safe_stop && !freeze_request_safe_stop_with_issue {
            queue_state::ApparatusQueueAction::DetachRoll
        } else {
            queue_action
        };
        progress.freeze_with_issue = freeze_with_issue;
        if freeze_with_issue {
            if !actor.role.trim().eq_ignore_ascii_case("aparatchi")
                && actor.role != "rezka_master"
            {
                return Err(ProductionMapError::OrderControlActionNotAllowed);
            }
            if progress.description.trim().is_empty()
                || !progress.freeze_request_id.trim().is_empty()
                || progress.worker_handoff
                || progress.remove_roll_from_apparatus
            {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            match control.state {
                OrderControlState::Active => {}
                OrderControlState::FreezeRequested => {
                    return Err(ProductionMapError::OrderFreezeRequested);
                }
                OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
            }
        }
        validate_freeze_request_pause(
            &control,
            apparatus,
            requested_action,
            actor,
            &progress.freeze_request_id,
        )?;
        match control.state {
            OrderControlState::Active => {}
            OrderControlState::FreezeRequested
                if queue_action == queue_state::ApparatusQueueAction::Freeze => {}
            OrderControlState::FreezeRequested => {
                return Err(ProductionMapError::OrderFreezeRequested);
            }
            OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
        }
        Ok(QueueActionIntent {
            hold_stage,
            control,
            queue_action,
            progress_action,
            freeze_request_finalization,
            freeze_request_safe_stop,
            freeze_request_safe_stop_has_output,
            freeze_request_safe_stop_with_issue,
            freeze_with_issue,
            resume_pause_allowed,
        })
    }

    async fn freeze_control_update_for_action(
        &self,
        event: &ApparatusQueueActionEvent,
        session: Option<&OrderRunSession>,
        control: OrderControlRecord,
        freeze_with_issue: bool,
    ) -> Result<Option<OrderControlRecord>, ProductionMapError> {
        let order_id = event.order_id.as_str();
        let update = if freeze_with_issue {
            let session = session
                .ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
            if session.stage_node_id.trim().is_empty() {
                return Err(ProductionMapError::OrderFreezeTargetNotFound);
            }
            let now = progress::unix_seconds();
            Some(OrderControlRecord {
                stage_node_id: session.stage_node_id.clone(),
                order_id: order_id.to_string(),
                state: OrderControlState::Frozen,
                actor: event.actor.clone(),
                requested_at_unix: now,
                frozen_at_unix: Some(now),
                early_close: None,
                freeze_request: Some(OrderFreezeRequest {
                    scope: "stage".to_string(),
                    stage_node_id: session.stage_node_id.clone(),
                    request_id: format!("order-freeze-issue:{}", event.event_id),
                    status: OrderFreezeRequestStatus::Frozen,
                    target_session_id: session.session_id.clone(),
                    target_apparatus: event.apparatus.clone(),
                    target_worker_role: event.actor.role.trim().to_string(),
                    target_worker_ref: event.actor.ref_.trim().to_string(),
                    target_worker_display_name: event.actor.display_name.trim().to_string(),
                    requested_at_unix: now,
                    transitioned_at_unix: now,
                }),
            })
        } else if control.state == OrderControlState::FreezeRequested
            && event.action == queue_state::ApparatusQueueAction::Freeze
        {
            let now = progress::unix_seconds();
            let mut freeze_request = control
                .freeze_request
                .ok_or(ProductionMapError::OrderFreezeRequestMismatch)?;
            let wildcard = control.stage_node_id.is_empty() && freeze_request.target_apparatus.is_empty();
            let others_running = wildcard && (
                self.store.order_run_sessions_for_order(order_id).await?.iter()
                    .any(|s| s.status == OrderRunStatus::Active && s.apparatus != event.apparatus)
                || self.store.active_print_preflight_holds().await?.iter().any(|h|
                    h.order_id == order_id && h.apparatus != event.apparatus
                    && matches!(h.status, PrintPreflightStatus::Held | PrintPreflightStatus::Running)));
            freeze_request.status = if others_running { OrderFreezeRequestStatus::Pending } else { OrderFreezeRequestStatus::Frozen };
            freeze_request.transitioned_at_unix = now;
            Some(OrderControlRecord {
                stage_node_id: control.stage_node_id.clone(),
                order_id: order_id.to_string(),
                state: if others_running { OrderControlState::FreezeRequested } else { OrderControlState::Frozen },
                actor: control.actor,
                requested_at_unix: control.requested_at_unix,
                frozen_at_unix: (!others_running).then_some(now),
                early_close: control.early_close.map(|mut close| {
                    close.closed_at_unix = (!others_running).then_some(now);
                    close
                }),
                freeze_request: Some(freeze_request),
            })
        } else {
            None
        };
        Ok(update)
    }

    async fn set_partial_station_completion(
        &self,
        event: &ApparatusQueueActionEvent,
        order_map: &ProductionMapDefinition,
        canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
        all_states: &ApparatusQueueStateMap,
        completion_read_snapshot: Option<&ProgressBuildReadSnapshot>,
        progress: &mut QueueProgressInput,
    ) -> Result<(), ProductionMapError> {
        let order_id = event.order_id.as_str();
        if event.action == queue_state::ApparatusQueueAction::Complete
            && (apparatus::uses_lamination_workflow(canonical)
                || apparatus::is_rezka_apparatus(canonical))
            && !progress.force_full_completion_metrics
        {
            let read_snapshot = completion_read_snapshot
                .ok_or(ProductionMapError::StoreFailed)?;
            let input_batch_id = Self::completion_input_batch_id_from_snapshot(
                progress,
                read_snapshot,
            );
            progress.allow_partial_station_completion = has_available_previous_wips_for_local_report(
                order_id,
                order_map,
                &event.apparatus,
                canonical,
                all_states,
                &read_snapshot.progress_batches,
                &[],
                &read_snapshot.opening_wip_records,
                &[],
                &input_batch_id,
                &event.stage_node_id,
                {
                    let sessions = self.store.order_run_sessions_for_order(order_id).await?;
                    let statuses = super::super::stage_execution::stage_work_statuses(order_map, &sessions,
                        &super::super::stage_execution::work_inputs(order_map, &read_snapshot.progress_batches, &read_snapshot.opening_wip_records), all_states, &[]);
                    super::super::stage_execution::work_control(order_map, &event.apparatus, &event.stage_node_id, &statuses, &sessions)
                        .map(|s| s.upstream_closed)
                },
            );
        }
        Ok(())
    }

    async fn previous_progress_ready_for_action(
        &self,
        action: queue_state::ApparatusQueueAction,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress: &QueueProgressInput,
    ) -> Result<bool, ProductionMapError> {
        if action != queue_state::ApparatusQueueAction::Start {
            return Ok(false);
        }
        if self
            .opening_wip_start_batch(order_id, order_map, apparatus, progress)
            .await?
            .is_some()
        {
            return Ok(true);
        }
        Ok(self
            .previous_stage_start_progress_batch(order_id, order_map, apparatus, progress)
            .await?
            .is_some())
    }
}

fn attach_queue_progress_witnesses(
    event: &mut ApparatusQueueActionEvent,
    progress: &mut QueueProgressRecords,
    framed_rezka_output: bool,
    include_bobina_in_net: Option<bool>,
    local_report_submitted: bool,
    closing_output_batch_id: Option<&str>,
) {
    if framed_rezka_output
        && progress.progress_batch.iter().chain(&progress.progress_batches).any(|batch| {
            batch.payload_json["rezka_output_kind"].as_str() == Some("frame")
        })
    {
        event.payload_json["rezka_expected_bobina_mode"] =
            serde_json::json!(include_bobina_in_net);
    }
    if let Some(waste) = progress.progress_event.as_ref()
        .and_then(|event| event.payload_json.get("zero_output_waste"))
    {
        event.payload_json["zero_output_waste"] = waste.clone();
    }
    if let Some(session) = &mut progress.session {
        session.payload_json[super::super::stage_execution::WORK_PROTOCOL] = serde_json::json!(1);
        // This intent is assigned a durable, order-scoped sequence in the
        // same transaction as output and WIP ownership changes.
        if local_report_submitted {
            event.payload_json["stage_work_report_submitted"] = serde_json::json!(true);
        }
    }
    // Resume can claim a returned lamination input just like Start/Merge.
    // Forward its trusted witnesses to the queue write so the transaction
    // can revalidate the source and route before changing ownership.
    if matches!(event.action, queue_state::ApparatusQueueAction::Start | queue_state::ApparatusQueueAction::Merge | queue_state::ApparatusQueueAction::Resume)
        && let Some(input_event) = &progress.progress_event
    {
        for key in ["wip_input_map_fingerprint", "wip_input_expected_batch",
            super::super::temporary_pause::EXPECTED_SESSION, "temporary_pause_held_inputs", "temporary_pause_map_fingerprint"] {
            if let Some(value) = input_event.payload_json.get(key) {
                event.payload_json[key] = value.clone();
            }
        }
    }
    if let Some(batch_id) = closing_output_batch_id {
        if let Some(session) = &mut progress.session {
            session.payload_json["complete_without_output"] = serde_json::json!(true);
            session.payload_json["closing_output_batch_id"] = serde_json::json!(batch_id);
        }
        if let Some(progress_event) = &mut progress.progress_event {
            progress_event.payload_json["complete_without_output"] = serde_json::json!(true);
            progress_event.payload_json["closing_output_batch_id"] = serde_json::json!(batch_id);
        }
    }
    if let Some(revision) = event.payload_json.get("rezka_expected_output_revision")
        .and_then(serde_json::Value::as_u64)
        && let Some(session) = &mut progress.session
    {
        session.payload_json["rezka_output_revision"] = serde_json::json!(revision + 1);
    }
    if event.stage_node_id.is_empty()
        && let Some(stage_node_id) = progress
            .session
            .as_ref()
            .map(|session| session.stage_node_id.trim())
            .filter(|value| !value.is_empty())
    {
        event.stage_node_id = stage_node_id.to_string();
    }
}

fn ensure_previous_stage_is_configured(
    action: queue_state::ApparatusQueueAction,
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
) -> Result<(), ProductionMapError> {
    if action != queue_state::ApparatusQueueAction::Freeze
        && apparatus::requires_previous_stage(canonical)
        && chain::previous_stage_resolution_is_unavailable(order_map, apparatus)
    {
        return Err(ProductionMapError::ProgressQrRequired);
    }
    Ok(())
}
