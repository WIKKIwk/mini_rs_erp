async fn required_existing_order_id(
    service: &ProductionMapService,
    order_id: &str,
) -> Result<String, ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    if !order_id.starts_with("zakaz-") {
        return Err(ProductionMapError::MapNotFound);
    }
    if !service
        .store
        .map_by_id(order_id)
        .await?
        .is_some()
    {
        return Err(ProductionMapError::MapNotFound);
    }
    Ok(order_id.to_string())
}

async fn current_order_control(
    service: &ProductionMapService,
    order_id: &str,
) -> Result<OrderControlRecord, ProductionMapError> {
    Ok(service
        .store
        .order_control_by_id(order_id)
        .await?
        .unwrap_or_else(|| OrderControlRecord::active(order_id)))
}

async fn recovery_freeze_request(
    service: &ProductionMapService,
    current: &OrderControlRecord,
    order_id: &str,
    now: i64,
) -> Result<OrderFreezeRequest, ProductionMapError> {
    let queue_states = service.store.apparatus_queue_states().await?;
    let sessions = service.store.order_run_sessions_for_order(order_id).await?;
    let session = sessions
        .iter()
        .find(|session| session.status == OrderRunStatus::Frozen);
    let mut target_apparatus = session
        .map(|session| session.apparatus.trim().to_string())
        .or_else(|| {
            queue_states.iter().find_map(|(apparatus, states)| {
                (states
                    .get(order_id)
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    == Some(queue_state::ApparatusQueueOrderState::Frozen))
                .then(|| apparatus.trim().to_string())
            })
        })
        .filter(|apparatus| !apparatus.is_empty())
        .ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
    let actor = session
        .map(|session| QueueActionActor {
            role: session.worker_role.clone(),
            ref_: session.worker_ref.clone(),
            display_name: session.worker_display_name.clone(),
        })
        .unwrap_or_else(|| QueueActionActor { role: String::new(), ref_: String::new(), display_name: String::new() });
    if session.is_none() {
        // A legacy queue-only stop has no worker recipient. The restore
        // wrapper recovers every frozen queue under a global wildcard.
        target_apparatus.clear();
    }
    Ok(OrderFreezeRequest {
        scope: "order".to_string(),
        stage_node_id: String::new(),
        request_id: format!("order-freeze-recovery:{order_id}"),
        status: OrderFreezeRequestStatus::Unfrozen,
        target_session_id: session
            .map(|session| session.session_id.trim().to_string())
            .unwrap_or_default(),
        target_apparatus,
        target_worker_role: actor.role,
        target_worker_ref: actor.ref_,
        target_worker_display_name: actor.display_name,
        requested_at_unix: current.requested_at_unix.max(0),
        transitioned_at_unix: now,
    })
}

async fn order_flow_evidence(
    service: &ProductionMapService,
    order_id: &str,
) -> Result<OrderFlowEvidence, ProductionMapError> {
    let all_states = service.store.apparatus_queue_states().await?;
    let sessions = service.store.order_run_sessions_for_order(order_id).await?;
    let batches = service.store.progress_batches_for_order(order_id).await?;
    let print_preflight_holds = service
        .store
        .active_print_preflight_holds()
        .await?
        .into_iter()
        .filter(|hold| hold.order_id.trim() == order_id.trim())
        .collect::<Vec<_>>();
    let logs = service
        .store
        .queue_action_logs_for_orders(&[order_id.to_string()])
        .await?
        .remove(order_id)
        .unwrap_or_default();

    let mut started_apparatuses = BTreeSet::new();
    let mut non_pending_state = false;
    let mut has_active_work = false;
    for (apparatus, states) in &all_states {
        let Some(state) = states
            .get(order_id)
            .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
        else {
            continue;
        };
        if state != queue_state::ApparatusQueueOrderState::Pending {
            non_pending_state = true;
            started_apparatuses.insert(apparatus.clone());
        }
        if state == queue_state::ApparatusQueueOrderState::InProgress {
            has_active_work = true;
        }
    }
    for hold in &print_preflight_holds {
        if matches!(hold.status, PrintPreflightStatus::Held | PrintPreflightStatus::Running) {
            has_active_work = true;
        }
    }
    let active_sessions = sessions
        .iter()
        .filter(|session| session.status == OrderRunStatus::Active)
        .cloned()
        .collect::<Vec<_>>();
    let paused_sessions = sessions
        .iter()
        .filter(|session| {
            matches!(
                session.status,
                OrderRunStatus::Paused | OrderRunStatus::RollDetached
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    for session in &sessions {
        started_apparatuses.insert(session.apparatus.trim().to_string());
        if session.status == OrderRunStatus::Active {
            has_active_work = true;
        }
    }
    for batch in &batches {
        started_apparatuses.insert(batch.apparatus.trim().to_string());
    }
    for log in &logs {
        started_apparatuses.insert(log.apparatus.trim().to_string());
    }
    started_apparatuses.retain(|value| !value.is_empty());

    let status = service.order_status_detail(order_id).await?;
    let completed = matches!(
        status.order_status.as_str(),
        "completed" | "completed_with_issue"
    );
    let started =
        non_pending_state || !sessions.is_empty() || !batches.is_empty() || !logs.is_empty();
    Ok(OrderFlowEvidence {
        started,
        completed,
        has_active_work,
        started_apparatuses,
        active_sessions,
        paused_sessions,
        print_preflight_holds,
    })
}

fn new_freeze_request_id() -> String {
    let bytes: [u8; 12] = rand::random();
    format!(
        "order-freeze-request_{}",
        data_encoding::HEXLOWER.encode(&bytes)
    )
}


async fn prepare_direct_freeze_queue_write(
    service: &ProductionMapService,
    record: &OrderControlRecord,
    target_session: Option<&OrderRunSession>,
    target_preflight: Option<&PrintPreflightHold>,
) -> Result<Option<QueueActionProgressWrite>, ProductionMapError> {
    let all_states = service.store.apparatus_queue_states().await?;
    let sequences = service.store.apparatus_sequences().await?;
    let order_controls = service.store.order_control_states().await?;
    let maps = service.store.maps().await?;
    let target_apparatus = target_session
        .map(|session| session.apparatus.trim().to_string())
        .or_else(|| target_preflight.map(|hold| hold.apparatus.trim().to_string()))
        .or_else(|| if record.stage_node_id.is_empty() { None } else {
            record.freeze_request.as_ref().map(|r| r.target_apparatus.clone())
        })
        .or_else(|| {
            all_states.iter().find_map(|(apparatus, states)| {
                (states
                    .get(record.order_id.trim())
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    == Some(queue_state::ApparatusQueueOrderState::Paused))
                .then(|| apparatus.clone())
            })
        });
    let Some(target_apparatus) = target_apparatus else {
        return Ok(None);
    };
    let storage_key = queue_state::apparatus_search_key(&target_apparatus);
    let canonical = service
        .resolve_canonical_apparatus_text(&storage_key)
        .await?;
    let mut parsed = all_states
        .get(&storage_key)
        .or_else(|| all_states.get(&target_apparatus))
        .map(parsed_queue_states)
        .unwrap_or_default();
    let from_state = parsed
        .get(record.order_id.trim())
        .copied()
        .unwrap_or(queue_state::ApparatusQueueOrderState::Paused);
    let requeued_paused_session = from_state == queue_state::ApparatusQueueOrderState::Pending
        && target_session.is_some_and(|session| {
            matches!(
                session.status,
                OrderRunStatus::Paused | OrderRunStatus::RollDetached
            )
        });
    if from_state == queue_state::ApparatusQueueOrderState::PrintPreflight
        && target_preflight
            .is_none_or(|hold| hold.status != PrintPreflightStatus::Passed)
    {
        return Ok(None);
    }
    if !matches!(
        from_state,
        queue_state::ApparatusQueueOrderState::Paused
            | queue_state::ApparatusQueueOrderState::Frozen
            | queue_state::ApparatusQueueOrderState::PrintPreflight
    ) && !requeued_paused_session
        && !(from_state == queue_state::ApparatusQueueOrderState::Pending && !record.stage_node_id.is_empty())
    {
        return Ok(None);
    }
    parsed.insert(
        record.order_id.clone(),
        queue_state::ApparatusQueueOrderState::Frozen,
    );
    let visible_order_ids = visible_order_ids_for_apparatus(&maps, &target_apparatus);
    let stored_sequence = sequences
        .get(&storage_key)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut excluded_order_ids = order_controls
        .iter()
        .filter_map(|(id, control)| {
            (control.state == OrderControlState::Frozen).then_some(id.clone())
        })
        .collect::<BTreeSet<_>>();
    excluded_order_ids.insert(record.order_id.clone());
    let sequence = queue_state::effective_apparatus_sequence_excluding(
        stored_sequence,
        &visible_order_ids,
        &excluded_order_ids,
    );
    let mut sequence_updates =
        sequence_updates_for_frozen_transition(&maps, &sequences, &excluded_order_ids, None);
    if !record.stage_node_id.is_empty() {
        sequence_updates.retain(|apparatus, _| queue_state::apparatus_ids_match(apparatus, &target_apparatus));
    }
    let policy = effective_apparatus_queue_policy(canonical.as_ref());
    let actor = record.actor.clone();
    let mut event = queue_action_event(QueueActionEventInput {
        requested_apparatus: &target_apparatus,
        storage_key: &storage_key,
        order_id: &record.order_id,
        stage_node_id: target_session
            .map(|session| session.stage_node_id.trim())
            .or_else(|| target_preflight.map(|hold| hold.stage_node_id.trim()))
            .unwrap_or(&record.stage_node_id),
        action: queue_state::ApparatusQueueAction::Freeze,
        from_state,
        to_state: queue_state::ApparatusQueueOrderState::Frozen,
        policy,
        actor: &actor,
        assigned_apparatus: &[],
        sequence: &sequence,
        visible_order_ids: &visible_order_ids,
    });
    event.event_id = queue_action_event_id(
        &storage_key,
        &record.order_id,
        queue_state::ApparatusQueueAction::Freeze,
    );
    event.payload_json["admin_freeze"] = serde_json::json!(true);
    event.payload_json["order_control_state"] = serde_json::json!(record.state.as_str());
    if target_preflight.is_some() {
        event.payload_json["print_preflight_passed_freeze"] = serde_json::json!(true);
    }
    let session = target_session.map(|session| {
        let mut payload_json = session.payload_json.clone();
        if !payload_json.is_object() {
            payload_json = serde_json::json!({});
        }
        payload_json["frozen_order"] = serde_json::json!(true);
        payload_json["admin_freeze"] = serde_json::json!(true);
        let mut frozen = OrderRunSession {
            status: OrderRunStatus::Frozen,
            updated_at_unix: unix_seconds(),
            payload_json,
            ..session.clone()
        };
        frozen.release_frozen_qolips();
        frozen
    });
    Ok(Some(QueueActionProgressWrite {
        apparatus: storage_key,
        map_update: None,
        states: serialized_queue_states(parsed),
        sequence_updates,
        event,
        session,
        progress_event: None,
        progress_batch: None,
        progress_batches: Vec::new(),
        progress_batch_updates: Vec::new(),
        opening_wip_batch_updates: Vec::new(),
        raw_material_stock_transitions: Vec::new(),
        qolip_checkouts: Vec::new(),
        returned_paint_report: None,
        order_control_update: Some(record.clone()),
        schedule_reservation_status: Some(ApparatusScheduleStatus::Paused),
        print_preflight_hold_id: None,
        print_preflight_cancel_hold_id: target_preflight
            .map(|hold| hold.hold_id.trim().to_string()),
    }))
}

async fn restore_frozen_queue_after_unfreeze(
    service: &ProductionMapService,
    record: &OrderControlRecord,
) -> Result<(), ProductionMapError> {
    if !record.stage_node_id.is_empty() {
        return restore_one_frozen_queue(service, record, true).await;
    }
    let locals = service.store.stage_order_control_states().await?;
    let sessions = service.store.order_run_sessions_for_order(&record.order_id).await?;
    let states = service.store.apparatus_queue_states().await?;
    let mut targets = BTreeMap::<String, Option<&OrderRunSession>>::new();
    for session in &sessions {
        if session.status == OrderRunStatus::Frozen
            && !locals.iter().any(|c| c.order_id == record.order_id
                && c.stage_node_id == session.stage_node_id && c.state != OrderControlState::Active
                && c.freeze_request.as_ref().is_some_and(|r| r.target_apparatus == session.apparatus)) {
            targets.insert(session.apparatus.clone(), Some(session));
        }
    }
    for (apparatus, entries) in states {
        if entries.get(&record.order_id).is_some_and(|s| s == "frozen")
            && !locals.iter().any(|c| c.order_id == record.order_id && c.state != OrderControlState::Active
                && c.freeze_request.as_ref().is_some_and(|r| r.target_apparatus == apparatus)) {
            targets.entry(apparatus).or_insert(None);
        }
    }
    // Keep the global overlay until every physical queue has been restored.
    // Retrying after a partial failure simply restores the remaining targets.
    for (apparatus, session) in targets {
        let mut target = record.clone();
        let request = target.freeze_request.as_mut().ok_or(ProductionMapError::OrderFreezeRequestMismatch)?;
        request.target_apparatus = apparatus;
        request.target_session_id = session.map(|s| s.session_id.clone()).unwrap_or_default();
        restore_one_frozen_queue(service, &target, false).await?;
    }
    service.store.put_order_control_state(record.clone()).await?;
    Ok(())
}
