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
    if !action.creates_resumable_output()
        && action != queue_state::ApparatusQueueAction::Freeze
    {
        return Ok(());
    }
    let request = control
        .freeze_request
        .as_ref()
        .ok_or(ProductionMapError::OrderFreezeRequestMismatch)?;
    let request_id_matches = !supplied_request_id.is_empty()
        && request.request_id.trim() == supplied_request_id
        && request.status == OrderFreezeRequestStatus::Pending;
    // Apparatus assignment is checked before entering this function. A
    // replacement worker of the same role may stop the targeted session;
    // target_worker_ref remains audit context, not an exclusive permission.
    let worker_role_matches = request.target_worker_role.trim() == actor.role.trim()
        && !actor.ref_.trim().is_empty();
    let apparatus_matches = (control.stage_node_id.is_empty() && request.target_apparatus.is_empty())
        || super::super::types::apparatus_ids_match(&request.target_apparatus, apparatus);
    if !request_id_matches || !worker_role_matches || !apparatus_matches {
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
    if !(control.stage_node_id.is_empty() && request.target_apparatus.is_empty())
        && (request.target_session_id.trim().is_empty()
        || request.target_session_id.trim() != session.session_id.trim())
    {
        return Err(ProductionMapError::OrderFreezeRequestMismatch);
    }
    Ok(())
}

fn preserve_freeze_work_owner(
    progress: &mut QueueProgressRecords,
    original: &OrderRunSession,
    actor: &QueueActionActor,
    owner: &serde_json::Value,
) {
    let performed_by = serde_json::json!({
        "role": actor.role, "ref": actor.ref_, "display_name": actor.display_name,
    });
    let stamp = |payload: &mut serde_json::Value| {
        if !payload.is_object() { *payload = serde_json::json!({}); }
        payload["freeze_session_owner"] = owner.clone();
        payload["freeze_performed_by"] = performed_by.clone();
    };
    if let Some(session) = &mut progress.session {
        session.worker_role.clone_from(&original.worker_role);
        session.worker_ref.clone_from(&original.worker_ref);
        session.worker_display_name.clone_from(&original.worker_display_name);
        stamp(&mut session.payload_json);
    }
    if let Some(event) = &mut progress.progress_event {
        stamp(&mut event.payload_json);
    }
    for batch in progress.progress_batch.iter_mut().chain(progress.progress_batches.iter_mut()) {
        batch.worker_role.clone_from(&original.worker_role);
        batch.worker_ref.clone_from(&original.worker_ref);
        batch.worker_display_name.clone_from(&original.worker_display_name);
        stamp(&mut batch.payload_json);
    }
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
