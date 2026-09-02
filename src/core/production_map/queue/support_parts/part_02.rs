
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
}

fn progress_batch_order_key(batch: &OrderProgressBatch) -> (u128, &str) {
    let stamp = batch
        .batch_id
        .split(':')
        .nth(1)
        .and_then(|value| value.parse::<u128>().ok())
        .unwrap_or_default();
    (stamp, batch.batch_id.trim())
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
    let mut current_positions = BTreeMap::new();
    for (index, id) in current_sequence.iter().enumerate() {
        current_positions.entry(id.trim()).or_insert(index);
    }
    let mut next_positions = BTreeMap::new();
    for (index, id) in next_sequence.iter().enumerate() {
        next_positions.entry(id.trim()).or_insert(index);
    }
    let mut prefix_max_current_position = Vec::with_capacity(next_sequence.len());
    let mut max_current_position: Option<usize> = None;
    let mut prefix_is_known = true;
    for id in next_sequence {
        if let Some(&current_position) = current_positions.get(id.trim()) {
            max_current_position = Some(
                max_current_position
                    .map_or(current_position, |max| max.max(current_position)),
            );
        } else {
            prefix_is_known = false;
        }
        prefix_max_current_position.push(if prefix_is_known {
            max_current_position
        } else {
            None
        });
    }

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
        let Some(&next_index) = next_positions.get(order_id) else {
            return Err(ProductionMapError::QueueActionNotAllowed);
        };
        let current_index = current_positions.get(order_id).copied().unwrap_or(0);
        if next_index > current_index {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        if next_index > 0 {
            match prefix_max_current_position[next_index - 1] {
                Some(max_position) if max_position < current_index => {}
                _ => return Err(ProductionMapError::QueueActionNotAllowed),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod active_sequence_barrier_tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn active(order_id: &str) -> BTreeMap<String, String> {
        BTreeMap::from([(
            order_id.to_string(),
            queue_state::ApparatusQueueOrderState::InProgress
                .as_str()
                .to_string(),
        )])
    }

    #[test]
    fn unchanged_active_prefix_is_allowed() {
        assert_eq!(
            validate_active_sequence_barrier(
                &ids(&["a", "b", "c"]),
                &ids(&["a", "b", "c"]),
                &active("b"),
                &BTreeSet::new(),
            ),
            Ok(())
        );
    }

    #[test]
    fn unknown_order_before_active_order_is_rejected() {
        assert_eq!(
            validate_active_sequence_barrier(
                &ids(&["a", "b", "c"]),
                &ids(&["unknown", "b", "a", "c"]),
                &active("b"),
                &BTreeSet::new(),
            ),
            Err(ProductionMapError::QueueActionNotAllowed)
        );
    }

    #[test]
    fn later_order_moved_before_active_order_is_rejected() {
        assert_eq!(
            validate_active_sequence_barrier(
                &ids(&["a", "b", "c"]),
                &ids(&["c", "b", "a"]),
                &active("b"),
                &BTreeSet::new(),
            ),
            Err(ProductionMapError::QueueActionNotAllowed)
        );
    }
}
