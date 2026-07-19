use std::collections::{BTreeMap, BTreeSet};

use super::*;

use super::progress::{effective_apparatus_queue_policy, queue_action_event_id};

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
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|key| key.to_string())
        .collect()
}

pub(super) fn queue_policy_for_apparatus(
    apparatus: &str,
    storage_key: &str,
    policies: &BTreeMap<String, ApparatusQueuePolicy>,
) -> ApparatusQueuePolicy {
    effective_apparatus_queue_policy(
        apparatus,
        policies
            .get(storage_key)
            .copied()
            .or_else(|| policies.get(apparatus).copied())
            .or_else(|| {
                policies.iter().find_map(|(key, policy)| {
                    queue_state::apparatus_titles_match(key, apparatus).then_some(*policy)
                })
            }),
    )
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

pub(super) fn append_laminatsiya_double_leftover_notice(
    action: queue_state::ApparatusQueueAction,
    batch: Option<&OrderProgressBatch>,
    order_map: &ProductionMapDefinition,
    event: &mut ApparatusQueueActionEvent,
) {
    if action != queue_state::ApparatusQueueAction::Complete {
        return;
    }
    let Some(batch) = batch else {
        return;
    };
    if batch.lamination_print_leftover_rolls.is_none()
        || batch.lamination_film_leftover_rolls.is_none()
    {
        return;
    }
    let print_leftover = batch.lamination_print_leftover_rolls.unwrap_or_default();
    let film_leftover = batch.lamination_film_leftover_rolls.unwrap_or_default();
    let total_waste = batch.total_waste.unwrap_or_default();
    let finished_kg = batch.finished_goods_kg.unwrap_or_default();
    let finished_meter = batch.finished_goods_meter.unwrap_or_default();
    event.payload_json["notice_kind"] =
        serde_json::Value::String("laminatsiya_double_leftover".to_string());
    event.payload_json["decision_required"] = serde_json::Value::Bool(false);
    event.payload_json["order_number"] =
        serde_json::Value::String(order_map.order_number.trim().to_string());
    event.payload_json["order_title"] =
        serde_json::Value::String(order_map.title.trim().to_string());
    event.payload_json["product_code"] =
        serde_json::Value::String(order_map.product_code.trim().to_string());
    event.payload_json["description"] = serde_json::Value::String(format!(
        "Laminatsiya tugatishda ikkala qavat qoldig'i yozildi. Bosmadan ortgan rulon: {print_leftover}. Plyonkadan ortgan rulon: {film_leftover}. Jami chiqindi: {total_waste} kg. Tayyor mahsulot: {finished_kg} kg, {finished_meter} m."
    ));
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
    batch.current_apparatus_key = queue_state::apparatus_search_key(&batch.current_apparatus);
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

pub(super) fn mark_finished_goods_batch_received(
    batch: &mut OrderProgressBatch,
    stock: &FinishedGoodsStockEntry,
    warehouse: &str,
    actor: &QueueActionActor,
    now: i64,
) {
    batch.wip_status = OrderProgressBatchWipStatus::Processed;
    batch.current_location = warehouse.to_string();
    batch.processed_by_session_id = stock.id.clone();
    batch.processed_by_apparatus = format!("warehouse:{warehouse}");
    batch.payload_json["received_warehouse"] = serde_json::json!(warehouse);
    batch.payload_json["received_by_role"] = serde_json::json!(actor.role.trim());
    batch.payload_json["received_by_ref"] = serde_json::json!(actor.ref_.trim());
    batch.payload_json["received_by_display_name"] = serde_json::json!(actor.display_name.trim());
    batch.payload_json["received_at_unix"] = serde_json::json!(now);
    batch.payload_json["finished_goods_stock_id"] = serde_json::json!(stock.id);
    batch.refresh_status_detail();
    batch.payload_json["status_detail"] = serde_json::json!(batch.status_detail);
    batch.payload_json["wip_status"] = serde_json::json!(batch.wip_status.as_str());
    batch.payload_json["current_location"] = serde_json::json!(batch.current_location);
    batch.payload_json["processed_by_session_id"] =
        serde_json::json!(batch.processed_by_session_id);
    batch.payload_json["processed_by_apparatus"] = serde_json::json!(batch.processed_by_apparatus);
}

fn progress_batch_order_key(batch: &OrderProgressBatch) -> (u128, String) {
    let stamp = batch
        .batch_id
        .split(':')
        .nth(1)
        .and_then(|value| value.parse::<u128>().ok())
        .unwrap_or_default();
    (stamp, batch.batch_id.trim().to_string())
}

pub(super) fn queue_states_for_order(
    queue_states: BTreeMap<String, BTreeMap<String, String>>,
    order_id: &str,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let order_id = order_id.trim();
    queue_states
        .into_iter()
        .filter_map(|(apparatus, states)| {
            states.get(order_id).map(|state| {
                (
                    apparatus,
                    BTreeMap::from([(order_id.to_string(), state.clone())]),
                )
            })
        })
        .collect()
}

pub(super) fn validate_active_sequence_barrier(
    current_sequence: &[String],
    next_sequence: &[String],
    states: &BTreeMap<String, String>,
) -> Result<(), ProductionMapError> {
    for (order_id, state) in states {
        let Some(parsed) = queue_state::ApparatusQueueOrderState::parse(state) else {
            continue;
        };
        if !parsed.is_active() {
            continue;
        }
        let order_id = order_id.trim();
        let Some(next_index) = next_sequence.iter().position(|id| id.trim() == order_id) else {
            return Err(ProductionMapError::QueueActionNotAllowed);
        };
        let current_index = current_sequence
            .iter()
            .position(|id| id.trim() == order_id)
            .unwrap_or(0);
        if next_index > current_index {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let allowed_before = current_sequence
            .iter()
            .take(current_index)
            .map(|id| id.trim())
            .collect::<BTreeSet<_>>();
        for id in next_sequence.iter().take(next_index) {
            if !allowed_before.contains(id.trim()) {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
        }
    }
    Ok(())
}
