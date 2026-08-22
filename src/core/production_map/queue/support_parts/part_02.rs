
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
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    order_id: &str,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let order_id = order_id.trim();
    queue_states
        .iter()
        .filter_map(|(apparatus, states)| {
            states.get(order_id).map(|state| {
                (
                    apparatus.clone(),
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
    frozen_order_ids: &BTreeSet<String>,
) -> Result<(), ProductionMapError> {
    for (order_id, state) in states {
        let Some(parsed) = queue_state::ApparatusQueueOrderState::parse(state) else {
            continue;
        };
        if !parsed.is_active() {
            continue;
        }
        let order_id = order_id.trim();
        if frozen_order_ids.contains(order_id) {
            continue;
        }
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
