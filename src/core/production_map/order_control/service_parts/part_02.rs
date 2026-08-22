
async fn prepare_direct_freeze_queue_write(
    service: &ProductionMapService,
    record: &OrderControlRecord,
    target_session: Option<&OrderRunSession>,
) -> Result<Option<QueueActionProgressWrite>, ProductionMapError> {
    let all_states = service.store.apparatus_queue_states().await?;
    let sequences = service.store.apparatus_sequences().await?;
    let order_controls = service.store.order_control_states().await?;
    let maps = service.store.maps().await?;
    let known_keys = known_apparatus_storage_keys(&sequences, &all_states);
    let target_apparatus = target_session
        .map(|session| session.apparatus.trim().to_string())
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
    let storage_key = queue_state::resolve_apparatus_storage_key(&target_apparatus, &known_keys);
    let canonical = service
        .resolve_canonical_apparatus_text(&storage_key)
        .await?;
    let mut parsed = parsed_queue_states(
        all_states
            .get(&storage_key)
            .or_else(|| all_states.get(&target_apparatus))
            .cloned()
            .unwrap_or_default(),
    );
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
    if !matches!(
        from_state,
        queue_state::ApparatusQueueOrderState::Paused
            | queue_state::ApparatusQueueOrderState::Frozen
    ) && !requeued_paused_session
    {
        return Ok(None);
    }
    parsed.insert(
        record.order_id.clone(),
        queue_state::ApparatusQueueOrderState::Frozen,
    );
    let visible_order_ids = visible_order_ids_for_apparatus(&maps, &target_apparatus);
    let stored_sequence = sequences.get(&storage_key).cloned().unwrap_or_default();
    let mut excluded_order_ids = order_controls
        .iter()
        .filter_map(|(id, control)| {
            (control.state == OrderControlState::Frozen).then_some(id.clone())
        })
        .collect::<BTreeSet<_>>();
    excluded_order_ids.insert(record.order_id.clone());
    let sequence = queue_state::effective_apparatus_sequence_excluding(
        &stored_sequence,
        &visible_order_ids,
        &excluded_order_ids,
    );
    let sequence_updates =
        sequence_updates_for_frozen_transition(&maps, &sequences, &excluded_order_ids, None);
    let policy = queue_policy_for_apparatus(canonical.as_ref());
    let actor = record.actor.clone();
    let mut event = queue_action_event(QueueActionEventInput {
        requested_apparatus: &target_apparatus,
        storage_key: &storage_key,
        order_id: &record.order_id,
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
    let session = target_session.map(|session| {
        let mut payload_json = session.payload_json.clone();
        if !payload_json.is_object() {
            payload_json = serde_json::json!({});
        }
        payload_json["frozen_order"] = serde_json::json!(true);
        payload_json["admin_freeze"] = serde_json::json!(true);
        OrderRunSession {
            status: OrderRunStatus::Frozen,
            updated_at_unix: unix_seconds(),
            payload_json,
            ..session.clone()
        }
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
        raw_material_stock_transitions: Vec::new(),
        qolip_checkouts: Vec::new(),
        returned_paint_report: None,
        order_control_update: Some(record.clone()),
        schedule_reservation_status: Some(ApparatusScheduleStatus::Paused),
    }))
}

async fn restore_frozen_queue_after_unfreeze(
    service: &ProductionMapService,
    record: &OrderControlRecord,
) -> Result<(), ProductionMapError> {
    let all_states = service.store.apparatus_queue_states().await?;
    let sequences = service.store.apparatus_sequences().await?;
    let sessions = service
        .store
        .order_run_sessions_for_order(&record.order_id)
        .await?;
    let known_keys = known_apparatus_storage_keys(&sequences, &all_states);
    let maps = service.store.maps().await?;
    let target_apparatus = record
        .freeze_request
        .as_ref()
        .map(|request| request.target_apparatus.trim())
        .filter(|apparatus| !apparatus.is_empty())
        .map(str::to_string)
        .or_else(|| {
            all_states.iter().find_map(|(apparatus, states)| {
                (states
                    .get(record.order_id.trim())
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    == Some(queue_state::ApparatusQueueOrderState::Frozen))
                .then(|| apparatus.clone())
            })
        })
        .or_else(|| {
            sessions.iter().find_map(|session| {
                (session.status == OrderRunStatus::Frozen).then(|| session.apparatus.clone())
            })
        })
        .ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
    let storage_key = queue_state::resolve_apparatus_storage_key(&target_apparatus, &known_keys);
    let canonical = service
        .resolve_canonical_apparatus_text(&storage_key)
        .await?;
    let mut parsed = parsed_queue_states(
        all_states
            .get(&storage_key)
            .or_else(|| all_states.get(&target_apparatus))
            .cloned()
            .unwrap_or_default(),
    );
    let from_state = parsed
        .get(record.order_id.trim())
        .copied()
        .unwrap_or(queue_state::ApparatusQueueOrderState::Frozen);
    let requested_session = record.freeze_request.as_ref().and_then(|request| {
        let target_session_id = request.target_session_id.trim();
        if target_session_id.is_empty() {
            return None;
        }
        sessions.iter().find(|session| {
            session.session_id.trim() == target_session_id
                && queue_state::apparatus_ids_match(&session.apparatus, &storage_key)
        })
    });
    let already_requeued_recovery = from_state == queue_state::ApparatusQueueOrderState::Pending
        && requested_session.is_some_and(|session| session.status == OrderRunStatus::Paused);
    if from_state != queue_state::ApparatusQueueOrderState::Frozen && !already_requeued_recovery {
        return Err(ProductionMapError::OrderControlActionNotAllowed);
    }
    parsed.insert(
        record.order_id.clone(),
        queue_state::ApparatusQueueOrderState::Pending,
    );

    let visible_order_ids = visible_order_ids_for_apparatus(&maps, &target_apparatus);
    let stored_sequence = sequences.get(&storage_key).cloned().unwrap_or_default();
    let frozen_order_ids = service
        .store
        .order_control_states()
        .await?
        .into_iter()
        .filter_map(|(id, control)| {
            (control.state == OrderControlState::Frozen && id != record.order_id).then_some(id)
        })
        .collect::<BTreeSet<_>>();
    let sequence = queue_state::effective_apparatus_sequence_excluding(
        &stored_sequence,
        &visible_order_ids,
        &frozen_order_ids,
    );
    let sequence_updates = sequence_updates_for_frozen_transition(
        &maps,
        &sequences,
        &frozen_order_ids,
        Some(&record.order_id),
    );
    let policy = queue_policy_for_apparatus(canonical.as_ref());
    let actor = record.actor.clone();
    let mut event = queue_action_event(QueueActionEventInput {
        requested_apparatus: &target_apparatus,
        storage_key: &storage_key,
        order_id: &record.order_id,
        action: queue_state::ApparatusQueueAction::Pause,
        from_state,
        to_state: queue_state::ApparatusQueueOrderState::Pending,
        policy,
        actor: &actor,
        assigned_apparatus: &[],
        sequence: &sequence,
        visible_order_ids: &visible_order_ids,
    });
    event.payload_json["admin_unfreeze"] = serde_json::json!(true);
    event.payload_json["requeued_at_tail"] = serde_json::json!(true);
    event.payload_json["order_control_state"] = serde_json::json!(record.state.as_str());
    if already_requeued_recovery {
        event.payload_json["recovered_control_only_freeze"] = serde_json::json!(true);
    }

    let session = requested_session
        .filter(|session| {
            session.status == OrderRunStatus::Frozen
                || (already_requeued_recovery && session.status == OrderRunStatus::Paused)
        })
        .or_else(|| {
            if already_requeued_recovery {
                None
            } else {
                sessions.iter().find(|session| {
                    session.status == OrderRunStatus::Frozen
                        && queue_state::apparatus_ids_match(&session.apparatus, &storage_key)
                })
            }
        })
        .map(unfrozen_order_run_session);

    service
        .store
        .put_apparatus_queue_states_with_event_and_progress(QueueActionProgressWrite {
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
            raw_material_stock_transitions: Vec::new(),
            qolip_checkouts: Vec::new(),
            returned_paint_report: None,
            order_control_update: Some(record.clone()),
            schedule_reservation_status: Some(ApparatusScheduleStatus::Paused),
        })
        .await?;
    Ok(())
}

fn unfrozen_order_run_session(session: &OrderRunSession) -> OrderRunSession {
    let mut payload_json = session.payload_json.clone();
    if !payload_json.is_object() {
        payload_json = serde_json::json!({});
    }
    let payload = payload_json
        .as_object_mut()
        .expect("object payload initialized above");
    payload.remove("frozen_order");
    payload.remove("admin_freeze");
    payload.insert("requeued_at_tail".to_string(), serde_json::json!(true));
    payload.insert(
        "unfrozen_at_unix".to_string(),
        serde_json::json!(unix_seconds()),
    );
    OrderRunSession {
        status: OrderRunStatus::Paused,
        updated_at_unix: unix_seconds(),
        payload_json,
        ..session.clone()
    }
}
