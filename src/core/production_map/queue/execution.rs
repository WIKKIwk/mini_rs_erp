include!("execution_impl_parts/part_01.rs");
include!("execution_impl_parts/part_02.rs");

fn schedule_reservation_status_for_action(
    action: queue_state::ApparatusQueueAction,
) -> Option<ApparatusScheduleStatus> {
    Some(match action {
        queue_state::ApparatusQueueAction::Start | queue_state::ApparatusQueueAction::Resume => {
            ApparatusScheduleStatus::Active
        }
        queue_state::ApparatusQueueAction::Pause
        | queue_state::ApparatusQueueAction::Freeze
        | queue_state::ApparatusQueueAction::DetachRoll => ApparatusScheduleStatus::Paused,
        queue_state::ApparatusQueueAction::Merge => ApparatusScheduleStatus::Active,
        queue_state::ApparatusQueueAction::RollComplete => ApparatusScheduleStatus::Active,
        queue_state::ApparatusQueueAction::Complete => ApparatusScheduleStatus::Completed,
    })
}

fn validate_freeze_request_pause(
    control: &OrderControlRecord,
    apparatus: &str,
    action: queue_state::ApparatusQueueAction,
    actor: &QueueActionActor,
    supplied_request_id: &str,
) -> Result<(), ProductionMapError> {
    let supplied_request_id = supplied_request_id.trim();
    if control.state != OrderControlState::FreezeRequested {
        if supplied_request_id.is_empty() {
            return Ok(());
        }
        return Err(ProductionMapError::OrderFreezeRequestMismatch);
    }
    if !matches!(
        action,
        queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::DetachRoll
            | queue_state::ApparatusQueueAction::Freeze
    ) {
        return Ok(());
    }
    let request = control
        .freeze_request
        .as_ref()
        .ok_or(ProductionMapError::OrderFreezeRequestMismatch)?;
    let request_id_matches = !supplied_request_id.is_empty()
        && request.request_id.trim() == supplied_request_id
        && request.status == OrderFreezeRequestStatus::Pending;
    let worker_matches = request.target_worker_role.trim() == actor.role.trim()
        && request.target_worker_ref.trim() == actor.ref_.trim();
    let apparatus_matches =
        super::super::types::apparatus_ids_match(&request.target_apparatus, apparatus);
    if !request_id_matches || !worker_matches || !apparatus_matches {
        return Err(ProductionMapError::OrderFreezeRequestMismatch);
    }
    Ok(())
}

fn validate_freeze_request_target_session(
    control: &OrderControlRecord,
    active_session: Option<&OrderRunSession>,
) -> Result<(), ProductionMapError> {
    let request = control
        .freeze_request
        .as_ref()
        .ok_or(ProductionMapError::OrderFreezeRequestMismatch)?;
    let session = active_session.ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
    if request.target_session_id.trim().is_empty()
        || request.target_session_id.trim() != session.session_id.trim()
    {
        return Err(ProductionMapError::OrderFreezeRequestMismatch);
    }
    Ok(())
}

fn freeze_safe_stop_has_any_output(progress: &QueueProgressInput) -> bool {
    !progress.rezka_frames.is_empty()
        || progress.produced_qty.is_some()
        || progress.gross_qty.is_some()
        || progress.return_ink_kg.is_some()
        || progress.lamination_print_leftover_rolls.is_some()
        || progress.lamination_film_leftover_rolls.is_some()
        || progress.rezka_bosma_waste.is_some()
        || progress.rezka_lamination_waste.is_some()
        || progress.rezka_edge_waste.is_some()
        || progress.total_waste.is_some()
        || progress.finished_goods_kg.is_some()
        || progress.bobina_kg.is_some()
        || progress.finished_goods_meter.is_some()
        || progress.diameter.is_some()
}

fn freeze_safe_stop_output_is_complete(
    apparatus: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
    progress: &QueueProgressInput,
) -> bool {
    if apparatus::is_rezka_apparatus(apparatus) {
        return !progress.rezka_frames.is_empty()
            || (progress
                .produced_qty
                .or(progress.finished_goods_meter)
                .is_some()
                && progress.gross_qty.or(progress.finished_goods_kg).is_some()
                && progress.bobina_kg.is_some()
                && progress.diameter.is_some());
    }
    progress
        .produced_qty
        .or(progress.finished_goods_meter)
        .is_some()
        && progress.gross_qty.or(progress.finished_goods_kg).is_some()
        && progress.bobina_kg.is_some()
}

fn mark_freeze_request_safe_stop_progress(
    progress: &mut QueueProgressRecords,
    request_id: &str,
    with_issue: bool,
) {
    let request_id = request_id.trim();
    if let Some(session) = progress.session.as_mut() {
        session.status = OrderRunStatus::Frozen;
        if !session.payload_json.is_object() {
            session.payload_json = serde_json::json!({});
        }
        session.payload_json["frozen_order"] = serde_json::json!(true);
        session.payload_json["freeze_request_safe_stop"] = serde_json::json!(true);
        session.payload_json["freeze_request_id"] = serde_json::json!(request_id);
        if with_issue {
            session.payload_json["freeze_with_issue"] = serde_json::json!(true);
            let issue_note = session
                .payload_json
                .get("description")
                .cloned()
                .unwrap_or_else(|| serde_json::json!(""));
            session.payload_json["issue_note"] = issue_note;
        }
    }
    if let Some(event) = progress.progress_event.as_mut() {
        event.payload_json["freeze_request_safe_stop"] = serde_json::json!(true);
        event.payload_json["freeze_request_id"] = serde_json::json!(request_id);
        if with_issue {
            event.payload_json["freeze_with_issue"] = serde_json::json!(true);
            event.payload_json["issue_note"] = serde_json::json!(event.description.trim());
        }
    }
    let mark_batch = |batch: &mut OrderProgressBatch| {
        if !batch.payload_json.is_object() {
            batch.payload_json = serde_json::json!({});
        }
        batch.payload_json["freeze_request_safe_stop"] = serde_json::json!(true);
        batch.payload_json["freeze_request_id"] = serde_json::json!(request_id);
    };
    if let Some(batch) = progress.progress_batch.as_mut() {
        mark_batch(batch);
    }
    for batch in &mut progress.progress_batches {
        mark_batch(batch);
    }
}
