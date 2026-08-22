
pub(super) fn current_progress_batch_for_report(
    scanned_batch: &OrderProgressBatch,
    progress_batches: &[OrderProgressBatch],
) -> Option<OrderProgressBatch> {
    let mut current = scanned_batch.clone();
    let mut seen = BTreeSet::from([current.batch_id.trim().to_string()]);
    loop {
        let next = progress_batches
            .iter()
            .filter(|batch| batch.parent_batch_id.trim() == current.batch_id.trim())
            .max_by(|left, right| {
                progress_batch_order_key(left).cmp(&progress_batch_order_key(right))
            })
            .cloned();
        let Some(next) = next else {
            break;
        };
        if !seen.insert(next.batch_id.trim().to_string()) {
            break;
        }
        current = next;
    }
    Some(current)
}

pub(super) fn validate_queue_action_request(
    apparatus: &str,
    order_id: &str,
    assigned_apparatus: &[String],
) -> Result<(), ProductionMapError> {
    if apparatus.is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    if order_id.is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    if queue_state::apparatus_search_key(apparatus).is_empty() {
        return Err(ProductionMapError::ApparatusNotAssigned);
    }
    if !queue_state::apparatus_matches_assigned(apparatus, assigned_apparatus) {
        return Err(ProductionMapError::ApparatusNotAssigned);
    }
    Ok(())
}

pub(super) fn known_apparatus_storage_keys(
    sequences: &BTreeMap<String, Vec<String>>,
    all_states: &BTreeMap<String, BTreeMap<String, String>>,
) -> Vec<String> {
    sequences
        .keys()
        .chain(all_states.keys())
        .map(|key| key.as_str())
        .filter(|key| !queue_state::apparatus_search_key(key).is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|key| key.to_string())
        .collect()
}

pub(super) fn queue_policy_for_apparatus(
    canonical: &RuntimeApparatusConfiguration,
) -> ApparatusQueuePolicy {
    effective_apparatus_queue_policy(canonical)
}

pub(super) fn parsed_queue_states(
    states: BTreeMap<String, String>,
) -> BTreeMap<String, queue_state::ApparatusQueueOrderState> {
    states
        .into_iter()
        .filter_map(|(id, value)| {
            queue_state::ApparatusQueueOrderState::parse(&value).map(|state| (id, state))
        })
        .collect()
}

pub(super) fn order_run_session_was_requeued(session: &OrderRunSession) -> bool {
    session
        .payload_json
        .get("requeued_at_tail")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

pub(super) fn has_waiting_previous_stage_wip(
    batches: &[OrderProgressBatch],
    order_id: &str,
    previous_stage: &str,
    apparatus: &str,
) -> bool {
    batches.iter().any(|batch| {
        batch.order_id.trim() == order_id.trim()
            && super::types::apparatus_ids_match(&batch.apparatus, previous_stage)
            && matches!(
                batch.action,
                queue_state::ApparatusQueueAction::Pause
                    | queue_state::ApparatusQueueAction::DetachRoll
                    | queue_state::ApparatusQueueAction::RollComplete
                    | queue_state::ApparatusQueueAction::Complete
            )
            && matches!(
                batch.status,
                OrderProgressBatchStatus::Paused
                    | OrderProgressBatchStatus::RollDetached
                    | OrderProgressBatchStatus::Completed
                    | OrderProgressBatchStatus::Resumed
            )
            && (batch.next_apparatus.trim().is_empty()
                || super::types::stage_ids_match(&batch.next_apparatus, apparatus))
            && batch.wip_status == OrderProgressBatchWipStatus::Waiting
    })
}

pub(super) fn order_has_frozen_queue_state(
    states: &ApparatusQueueStateMap,
    order_id: &str,
) -> bool {
    let order_id = order_id.trim();
    !order_id.is_empty()
        && states.values().any(|apparatus_states| {
            apparatus_states
                .get(order_id)
                .and_then(|raw| queue_state::ApparatusQueueOrderState::parse(raw))
                == Some(queue_state::ApparatusQueueOrderState::Frozen)
        })
}

pub(super) fn sequence_updates_for_frozen_transition(
    maps: &[ProductionMapDefinition],
    sequences: &BTreeMap<String, Vec<String>>,
    excluded_order_ids: &BTreeSet<String>,
    appended_order_id: Option<&str>,
) -> BTreeMap<String, Vec<String>> {
    let visible_by_apparatus = visible_order_ids_by_apparatus(maps);
    let known_apparatus = sequences
        .keys()
        .chain(visible_by_apparatus.keys())
        .filter(|key| !queue_state::apparatus_search_key(key).is_empty())
        .cloned()
        .collect::<BTreeSet<_>>();
    let known_keys = known_apparatus.iter().cloned().collect::<Vec<_>>();
    let mut updates = BTreeMap::new();
    let mut seen_storage_keys = BTreeSet::new();

    for requested_apparatus in known_apparatus {
        let storage_key =
            queue_state::resolve_apparatus_storage_key(&requested_apparatus, &known_keys);
        if !seen_storage_keys.insert(storage_key.clone()) {
            continue;
        }
        let visible_order_ids = visible_order_ids_for_apparatus(maps, &requested_apparatus);
        if visible_order_ids.is_empty() {
            continue;
        }
        let stored_sequence = sequences
            .get(&storage_key)
            .or_else(|| sequences.get(&requested_apparatus))
            .cloned()
            .unwrap_or_default();
        let mut sequence = queue_state::effective_apparatus_sequence_excluding(
            &stored_sequence,
            &visible_order_ids,
            excluded_order_ids,
        );
        if let Some(order_id) = appended_order_id.map(str::trim).filter(|id| !id.is_empty()) {
            sequence.retain(|candidate| candidate.trim() != order_id);
            if visible_order_ids
                .iter()
                .any(|candidate| candidate.trim() == order_id)
            {
                sequence.push(order_id.to_string());
            }
        }
        updates.insert(storage_key, sequence);
    }
    updates
}

/// A malformed persisted state must never be treated as an absent state.
/// Treating it as `pending` would allow an operator to start an order while
/// the durable queue record is already corrupt. The audit endpoint reports
/// malformed states, while this guard keeps the live execution path safe.
pub(super) fn validate_requested_queue_state(
    states: &BTreeMap<String, String>,
    order_id: &str,
) -> Result<(), ProductionMapError> {
    let Some(raw_state) = states.get(order_id.trim()) else {
        return Ok(());
    };
    queue_state::ApparatusQueueOrderState::parse(raw_state)
        .map(|_| ())
        .ok_or(ProductionMapError::QueueActionNotAllowed)
}

pub(super) fn apply_queue_policy(
    policy: ApparatusQueuePolicy,
    previous_progress_ready: bool,
    sequence: &[String],
    parsed: &mut BTreeMap<String, queue_state::ApparatusQueueOrderState>,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
) -> Result<(), ProductionMapError> {
    match policy {
        ApparatusQueuePolicy::StrictSequence if !previous_progress_ready => {
            queue_state::apply_queue_action(sequence, parsed, order_id, action)
        }
        ApparatusQueuePolicy::StrictSequence | ApparatusQueuePolicy::FreePick => {
            queue_state::apply_unordered_queue_action(parsed, order_id, action)
        }
    }
}

pub(super) fn apply_requeued_resume(
    policy: ApparatusQueuePolicy,
    sequence: &[String],
    parsed: &mut BTreeMap<String, queue_state::ApparatusQueueOrderState>,
    order_id: &str,
) -> Result<(), ProductionMapError> {
    if policy == ApparatusQueuePolicy::StrictSequence
        && queue_state::first_actionable_order_id(sequence, parsed).as_deref() != Some(order_id)
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    if parsed
        .get(order_id)
        .copied()
        .unwrap_or(queue_state::ApparatusQueueOrderState::Pending)
        != queue_state::ApparatusQueueOrderState::Pending
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    parsed.insert(
        order_id.to_string(),
        queue_state::ApparatusQueueOrderState::InProgress,
    );
    Ok(())
}

pub(super) fn serialized_queue_states(
    parsed: BTreeMap<String, queue_state::ApparatusQueueOrderState>,
) -> BTreeMap<String, String> {
    parsed
        .into_iter()
        .map(|(id, state)| (id, state.as_str().to_string()))
        .collect()
}

pub(super) struct QueueActionEventInput<'a> {
    pub(super) requested_apparatus: &'a str,
    pub(super) storage_key: &'a str,
    pub(super) order_id: &'a str,
    pub(super) action: queue_state::ApparatusQueueAction,
    pub(super) from_state: queue_state::ApparatusQueueOrderState,
    pub(super) to_state: queue_state::ApparatusQueueOrderState,
    pub(super) policy: ApparatusQueuePolicy,
    pub(super) actor: &'a QueueActionActor,
    pub(super) assigned_apparatus: &'a [String],
    pub(super) sequence: &'a [String],
    pub(super) visible_order_ids: &'a [String],
}

pub(super) fn queue_action_event(input: QueueActionEventInput<'_>) -> ApparatusQueueActionEvent {
    ApparatusQueueActionEvent {
        event_id: queue_action_event_id(input.storage_key, input.order_id, input.action),
        apparatus: input.storage_key.to_string(),
        order_id: input.order_id.to_string(),
        action: input.action,
        from_state: input.from_state,
        to_state: input.to_state,
        policy: input.policy,
        actor: input.actor.clone(),
        assigned_apparatus: sanitized_assigned_apparatus(input.assigned_apparatus),
        payload_json: queue_action_event_payload(
            input.requested_apparatus,
            input.storage_key,
            input.sequence,
            input.visible_order_ids,
            input.from_state,
            input.to_state,
            input.policy,
        ),
    }
}

fn sanitized_assigned_apparatus(assigned_apparatus: &[String]) -> Vec<String> {
    assigned_apparatus
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn queue_action_event_payload(
    requested_apparatus: &str,
    storage_key: &str,
    sequence: &[String],
    visible_order_ids: &[String],
    from_state: queue_state::ApparatusQueueOrderState,
    to_state: queue_state::ApparatusQueueOrderState,
    policy: ApparatusQueuePolicy,
) -> serde_json::Value {
    serde_json::json!({
        "requested_apparatus": requested_apparatus,
        "storage_key": storage_key,
        "sequence": sequence,
        "visible_order_ids": visible_order_ids,
        "from_state": from_state.as_str(),
        "to_state": to_state.as_str(),
        "policy": policy.as_str(),
    })
}

pub(super) fn downgrade_completed_state_to_pending(
    order_id: &str,
    saved: &mut BTreeMap<String, String>,
    event: &mut ApparatusQueueActionEvent,
) {
    let to_state = queue_state::ApparatusQueueOrderState::Pending;
    saved.insert(order_id.to_string(), to_state.as_str().to_string());
    event.to_state = to_state;
    event.payload_json["to_state"] = serde_json::json!(to_state.as_str());
    event.payload_json["batch_complete_order_state"] = serde_json::json!("pending");
}

pub(super) fn finished_goods_qty_uom(
    batch: &OrderProgressBatch,
) -> Result<(f64, String), ProductionMapError> {
    if let Some(qty) = batch.finished_goods_kg
        && qty > 0.0
    {
        return Ok((qty, "kg".to_string()));
    }
    if let Some(qty) = batch.finished_goods_meter
        && qty > 0.0
    {
        return Ok((qty, "m".to_string()));
    }
    if batch.produced_qty > 0.0 && !batch.uom.trim().is_empty() {
        return Ok((batch.produced_qty, batch.uom.trim().to_string()));
    }
    Err(ProductionMapError::ProgressInputInvalid)
}

pub(super) fn progress_batch_needs_location_repair(batch: &OrderProgressBatch) -> bool {
    batch.current_apparatus.trim().is_empty() || batch.next_apparatus.trim().is_empty()
}

pub(super) fn repair_wip_progress_batch_locations(
    batches: &mut [OrderProgressBatch],
    maps_by_id: &BTreeMap<String, ProductionMapDefinition>,
) {
    for batch in batches {
        repair_current_apparatus_fields(batch);
        repair_next_apparatus_field(batch, maps_by_id);
    }
}

fn repair_current_apparatus_fields(batch: &mut OrderProgressBatch) {
    if !batch.current_apparatus.trim().is_empty() {
        return;
    }
    batch.current_apparatus = batch.apparatus.trim().to_string();
    batch.current_apparatus_key = super::types::canonical_apparatus_key(&batch.current_apparatus);
    if batch.current_location.trim().is_empty() {
        batch.current_location = batch.current_apparatus.clone();
    }
    batch.payload_json["current_apparatus"] = serde_json::json!(batch.current_apparatus);
    batch.payload_json["current_apparatus_key"] = serde_json::json!(batch.current_apparatus_key);
    batch.payload_json["current_location"] = serde_json::json!(batch.current_location);
}

fn repair_next_apparatus_field(
    batch: &mut OrderProgressBatch,
    maps_by_id: &BTreeMap<String, ProductionMapDefinition>,
) {
    if !batch.next_apparatus.trim().is_empty() {
        return;
    }
    let Some(map) = maps_by_id.get(batch.order_id.trim()) else {
        return;
    };
    if let Some(next) = chain::next_work_stage_station(map, &batch.current_apparatus) {
        batch.next_apparatus = next;
        batch.payload_json["next_apparatus"] = serde_json::json!(batch.next_apparatus);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn finished_goods_stock_entry(
    batch: &OrderProgressBatch,
    warehouse: &str,
    item_code: &str,
    item_name: &str,
    actor: &QueueActionActor,
    qty: f64,
    uom: String,
    now: i64,
) -> FinishedGoodsStockEntry {
    FinishedGoodsStockEntry {
        id: format!("finished:{}", batch.batch_id.trim()),
        warehouse: warehouse.to_string(),
        order_id: batch.order_id.trim().to_string(),
        item_code: item_code.trim().to_string(),
        item_name: item_name.trim().to_string(),
        qty,
        uom,
        status: "available".to_string(),
        barcode: batch.qr_payload.trim().to_string(),
        source_progress_batch_id: batch.batch_id.trim().to_string(),
        accepted_by_role: actor.role.trim().to_string(),
        accepted_by_ref: actor.ref_.trim().to_string(),
        accepted_by_display_name: actor.display_name.trim().to_string(),
        accepted_at_unix: now,
        payload_json: finished_goods_stock_payload(
            batch, warehouse, item_code, item_name, actor, now,
        ),
    }
}

fn finished_goods_stock_payload(
    batch: &OrderProgressBatch,
    warehouse: &str,
    item_code: &str,
    item_name: &str,
    actor: &QueueActionActor,
    now: i64,
) -> serde_json::Value {
    serde_json::json!({
        "source": "production_finished_goods_receipt",
        "progress_batch_id": batch.batch_id.trim(),
        "qr_payload": batch.qr_payload.trim(),
        "warehouse": warehouse,
        "order_id": batch.order_id.trim(),
        "item_code": item_code.trim(),
        "item_name": item_name.trim(),
        "accepted_by_role": actor.role.trim(),
        "accepted_by_ref": actor.ref_.trim(),
        "accepted_by_display_name": actor.display_name.trim(),
        "accepted_at_unix": now,
    })
}
