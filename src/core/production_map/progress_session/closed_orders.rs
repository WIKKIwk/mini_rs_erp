use std::collections::BTreeMap;

use super::super::chain;
use super::super::queue_state;
use super::super::types::{
    ProductionMapDefinition, ProductionOrderLifecycleStatus, ProductionOrderLogEntry,
    ProductionOrderOperationalStatus,
};

#[cfg(test)]
#[path = "stage_lifecycle_tests.rs"]
mod stage_lifecycle_tests;

pub(in crate::core::production_map) fn required_apparatus_for_closed_order(
    map: &ProductionMapDefinition,
) -> Option<Vec<String>> {
    chain::physical_work_stage_ids(map)
}

pub(in crate::core::production_map) fn order_completed_on_apparatus(
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    order_id: &str,
    apparatus: &str,
) -> bool {
    queue_states.iter().any(|(state_apparatus, states)| {
        super::super::types::apparatus_ids_match(state_apparatus, apparatus)
            && matches!(
                states
                    .get(order_id.trim())
                    .map(|value| value.trim().to_ascii_lowercase()),
                Some(state) if state == "completed"
            )
    })
}

pub(crate) fn derive_production_order_lifecycle(
    map: &ProductionMapDefinition,
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
) -> Option<ProductionOrderLifecycleStatus> {
    derive_production_order_lifecycle_with_stage_events(map, queue_states, &[])
}

/// Queue transitions in persistence order, not just individual roll completions.
/// `complete -> pending` means that the operation still has work remaining.
#[derive(Debug, Clone)]
pub(crate) struct ProductionStageLifecycleEvent {
    pub stage_node_id: String,
    pub action: queue_state::ApparatusQueueAction,
    pub to_state: queue_state::ApparatusQueueOrderState,
}

pub(crate) fn derive_production_order_lifecycle_with_stage_events(
    map: &ProductionMapDefinition,
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    stage_events: &[ProductionStageLifecycleEvent],
) -> Option<ProductionOrderLifecycleStatus> {
    // Do not silently omit an invalid physical stage from the completion check.
    required_apparatus_for_closed_order(map)?;
    let physical_stages = chain::linear_work_stages(map)
        .into_iter()
        .filter(|stage| stage.apparatus_id.is_some())
        .collect::<Vec<_>>();
    // A group identifies one operation, irrespective of which candidates did
    // its work. Never collapse repeated uses of an apparatus in other stages.
    let mut stages_by_operation = BTreeMap::new();
    let mut operation_by_node = BTreeMap::new();
    let mut occurrence_counts = BTreeMap::<&str, usize>::new();
    for stage in &physical_stages {
        let node = map
            .nodes
            .iter()
            .find(|node| node.id.trim() == stage.node_id.trim())?;
        let group = node.alternative_group_id.trim();
        let operation = if group.is_empty() {
            (false, node.id.trim())
        } else {
            (true, group)
        };
        operation_by_node.insert(node.id.trim(), operation);
        stages_by_operation
            .entry(operation)
            .or_insert_with(Vec::new)
            .push(stage);
        *occurrence_counts
            .entry(stage.apparatus_id.as_deref().unwrap_or_default())
            .or_default() += 1;
    }
    let mut latest_operation_events = BTreeMap::new();
    for event in stage_events {
        if let Some(operation) = operation_by_node.get(event.stage_node_id.trim()) {
            latest_operation_events.insert(*operation, event);
        }
    }
    let all_occurrences_completed = !stages_by_operation.is_empty()
        && stages_by_operation.iter().all(|(operation, stages)| {
            let apparatus = stages[0].apparatus_id.as_deref().unwrap_or_default();
            // Preserve the authoritative queue projection for a single plain
            // occurrence, including legacy state-only writes. It is ambiguous
            // for alternatives and repeated apparatus, which require history.
            if !operation.0
                && occurrence_counts.get(apparatus) == Some(&1)
                && queue_states
                    .get(apparatus)
                    .is_some_and(|orders| orders.contains_key(map.id.trim()))
            {
                return order_completed_on_apparatus(queue_states, &map.id, apparatus);
            }
            if let Some(event) = latest_operation_events.get(operation) {
                return event.action == queue_state::ApparatusQueueAction::Complete
                    && event.to_state == queue_state::ApparatusQueueOrderState::Completed;
            }
            // Never choose a candidate or use OR over apparatus states to
            // infer an alternative operation's end.
            false
        });
    if all_occurrences_completed {
        return Some(ProductionOrderLifecycleStatus::ProductionCompleted);
    }

    let has_started_operation = !latest_operation_events.is_empty()
        || queue_states.values().any(|states| {
            states.get(map.id.trim()).is_some_and(|state| {
                !state.trim().is_empty() && !state.trim().eq_ignore_ascii_case("pending")
            })
        });
    Some(if has_started_operation {
        ProductionOrderLifecycleStatus::InProgress
    } else {
        ProductionOrderLifecycleStatus::Released
    })
}

pub(crate) fn derive_production_order_operational_status(
    lifecycle_status: ProductionOrderLifecycleStatus,
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    order_id: &str,
    completed_with_issue_count: usize,
    has_roll_detached: bool,
    waiting_next_stage_count: usize,
) -> ProductionOrderOperationalStatus {
    if matches!(
        lifecycle_status,
        ProductionOrderLifecycleStatus::ProductionCompleted
            | ProductionOrderLifecycleStatus::Closed
    ) {
        return if completed_with_issue_count > 0 {
            ProductionOrderOperationalStatus::CompletedWithIssue
        } else {
            ProductionOrderOperationalStatus::Completed
        };
    }

    let states = queue_states
        .values()
        .filter_map(|states| states.get(order_id.trim()))
        .map(|state| state.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    if states.iter().any(|state| state == "frozen") {
        ProductionOrderOperationalStatus::Frozen
    } else if has_roll_detached || states.iter().any(|state| state == "in_progress") {
        ProductionOrderOperationalStatus::InProgress
    } else if states.iter().any(|state| state == "paused") {
        ProductionOrderOperationalStatus::Paused
    } else if states.iter().any(|state| state == "completed") {
        ProductionOrderOperationalStatus::PartiallyCompleted
    } else if waiting_next_stage_count > 0 {
        ProductionOrderOperationalStatus::WaitingNextStage
    } else if states.iter().any(|state| state == "pending") {
        ProductionOrderOperationalStatus::Ready
    } else {
        ProductionOrderOperationalStatus::NotStarted
    }
}

pub(in crate::core::production_map) fn latest_required_complete_event<'a>(
    map: &ProductionMapDefinition,
    logs: &'a [ProductionOrderLogEntry],
    required_apparatus: &[String],
) -> Option<&'a ProductionOrderLogEntry> {
    let stage_nodes = chain::linear_work_stages(map)
        .into_iter()
        .filter(|stage| stage.apparatus_id.is_some())
        .map(|stage| stage.node_id)
        .collect::<std::collections::BTreeSet<_>>();
    logs.iter()
        .filter(|entry| {
            entry.action == queue_state::ApparatusQueueAction::Complete
                && entry.to_state == queue_state::ApparatusQueueOrderState::Completed
                && if entry.stage_node_id.trim().is_empty() {
                    required_apparatus.iter().any(|apparatus| {
                        super::super::types::apparatus_ids_match(&entry.apparatus, apparatus)
                    })
                } else {
                    stage_nodes.contains(entry.stage_node_id.trim())
                }
        })
        .max_by_key(|entry| entry.created_at_unix)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        ProductionStageLifecycleEvent, derive_production_order_lifecycle,
        derive_production_order_lifecycle_with_stage_events,
        derive_production_order_operational_status, required_apparatus_for_closed_order,
    };
    use crate::core::production_map::{
        ProductionMapDefinition, ProductionOrderLifecycleStatus, ProductionOrderOperationalStatus,
    };

    fn completed_events(nodes: &[&str]) -> Vec<ProductionStageLifecycleEvent> {
        nodes
            .iter()
            .map(|node| ProductionStageLifecycleEvent {
                stage_node_id: (*node).to_string(),
                action: super::queue_state::ApparatusQueueAction::Complete,
                to_state: super::queue_state::ApparatusQueueOrderState::Completed,
            })
            .collect()
    }

    #[test]
    fn mixed_canonical_and_invalid_stage_fails_closed() {
        let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
            "id": "zakaz-closed-id",
            "product_code": "PRODUCT",
            "title": "Order",
            "nodes": [
                {
                    "id": "press",
                    "kind": "apparatus",
                    "title": "Renamed press",
                    "apparatus_id": "apparatus:catalog:press-001"
                },
                {
                    "id": "legacy",
                    "kind": "apparatus",
                    "title": "Legacy press",
                    "apparatus_id": "Legacy press"
                }
            ]
        }))
        .expect("closed-order map fixture");

        assert!(required_apparatus_for_closed_order(&map).is_none());
    }

    #[test]
    fn stale_legacy_title_is_not_used_as_apparatus_identity() {
        let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
            "id": "zakaz-legacy-stage",
            "product_code": "PRODUCT",
            "title": "Order",
            "nodes": [
                {
                    "id": "legacy",
                    "kind": "apparatus",
                    "title": "apparatus:catalog:press-001",
                    "apparatus_id": "Legacy press"
                }
            ]
        }))
        .expect("closed-order map fixture");

        assert!(required_apparatus_for_closed_order(&map).is_none());
    }

    #[test]
    fn all_valid_stages_are_retained_for_completion() {
        let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
            "id": "zakaz-valid-stages",
            "product_code": "PRODUCT",
            "title": "Order",
            "nodes": [
                {
                    "id": "press",
                    "kind": "apparatus",
                    "title": "Renamed press",
                    "apparatus_id": "apparatus:catalog:press-001"
                },
                {
                    "id": "lamination",
                    "kind": "apparatus",
                    "title": "Renamed lamination",
                    "apparatus_id": "apparatus:catalog:lam-001"
                }
            ]
        }))
        .expect("closed-order map fixture");

        assert_eq!(
            required_apparatus_for_closed_order(&map),
            Some(vec![
                "apparatus:catalog:press-001".to_string(),
                "apparatus:catalog:lam-001".to_string(),
            ])
        );
    }

    #[test]
    fn closure_uses_both_condition_branches_and_skips_virtual_tasks() {
        let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
            "id": "zakaz-branch-closure",
            "product_code": "PRODUCT",
            "title": "Order",
            "nodes": [
                {"id": "start", "kind": "start", "title": "Start"},
                {"id": "press", "kind": "apparatus", "title": "Press", "apparatus_id": "apparatus:catalog:press-001"},
                {"id": "condition", "kind": "condition", "title": "Condition"},
                {"id": "true_task", "kind": "task", "title": "True task"},
                {"id": "true_stage", "kind": "apparatus", "title": "True", "apparatus_id": "apparatus:catalog:true-001"},
                {"id": "false_stage", "kind": "apparatus", "title": "False", "apparatus_id": "apparatus:catalog:false-001"},
                {"id": "join", "kind": "apparatus", "title": "Join", "apparatus_id": "apparatus:catalog:join-001"},
                {"id": "end", "kind": "end", "title": "End"}
            ],
            "edges": [
                {"from": "start", "to": "press"},
                {"from": "press", "to": "condition"},
                {"from": "condition", "to": "true_task", "branch": "true"},
                {"from": "condition", "to": "false_stage", "branch": "false"},
                {"from": "true_task", "to": "true_stage"},
                {"from": "true_stage", "to": "join"},
                {"from": "false_stage", "to": "join"},
                {"from": "join", "to": "end"}
            ]
        }))
        .expect("branch closure map");

        assert_eq!(
            required_apparatus_for_closed_order(&map),
            Some(vec![
                "apparatus:catalog:press-001".to_string(),
                "apparatus:catalog:false-001".to_string(),
                "apparatus:catalog:true-001".to_string(),
                "apparatus:catalog:join-001".to_string(),
            ])
        );
    }

    #[test]
    fn lifecycle_completes_only_after_every_required_operation_is_completed() {
        let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
            "id": "zakaz-lifecycle",
            "product_code": "PRODUCT",
            "title": "Order",
            "nodes": [
                {"id": "start", "kind": "start", "title": "Start"},
                {"id": "press", "kind": "apparatus", "title": "Press", "apparatus_id": "apparatus:catalog:press-001"},
                {"id": "lamination", "kind": "apparatus", "title": "Lamination", "apparatus_id": "apparatus:catalog:lam-001"},
                {"id": "end", "kind": "end", "title": "End"}
            ],
            "edges": [
                {"from": "start", "to": "press"},
                {"from": "press", "to": "lamination"},
                {"from": "lamination", "to": "end"}
            ]
        }))
        .expect("lifecycle map fixture");
        let first_completed = BTreeMap::from([(
            "apparatus:catalog:press-001".to_string(),
            BTreeMap::from([("zakaz-lifecycle".to_string(), "completed".to_string())]),
        )]);
        let fully_completed = BTreeMap::from([
            (
                "apparatus:catalog:press-001".to_string(),
                BTreeMap::from([("zakaz-lifecycle".to_string(), "completed".to_string())]),
            ),
            (
                "apparatus:catalog:lam-001".to_string(),
                BTreeMap::from([("zakaz-lifecycle".to_string(), "completed".to_string())]),
            ),
        ]);

        assert_eq!(
            derive_production_order_lifecycle(&map, &BTreeMap::new()),
            Some(ProductionOrderLifecycleStatus::Released)
        );
        assert_eq!(
            derive_production_order_lifecycle(&map, &first_completed),
            Some(ProductionOrderLifecycleStatus::InProgress)
        );
        assert_eq!(
            derive_production_order_lifecycle(&map, &fully_completed),
            Some(ProductionOrderLifecycleStatus::ProductionCompleted)
        );
    }

    #[test]
    fn repeated_apparatus_requires_each_graph_occurrence_to_complete() {
        let order_id = "zakaz-repeated-rezka-lifecycle";
        let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
            "id": order_id,
            "product_code": "REZKA-REENTRY",
            "title": "Repeated Rezka",
            "nodes": [
                {"id": "start", "kind": "start", "title": "Start"},
                {"id": "bosma", "kind": "apparatus", "title": "Bosma", "apparatus_id": "apparatus:catalog:press-001"},
                {"id": "rezka_before_lamination", "kind": "apparatus", "title": "Rezka", "apparatus_id": "apparatus:default:asset-010"},
                {"id": "lamination", "kind": "apparatus", "title": "Laminatsiya", "apparatus_id": "apparatus:catalog:lam-001"},
                {"id": "rezka_final", "kind": "apparatus", "title": "Rezka", "apparatus_id": "apparatus:default:asset-010"},
                {"id": "end", "kind": "end", "title": "End"}
            ],
            "edges": [
                {"from": "start", "to": "bosma"},
                {"from": "bosma", "to": "rezka_before_lamination"},
                {"from": "rezka_before_lamination", "to": "lamination"},
                {"from": "lamination", "to": "rezka_final"},
                {"from": "rezka_final", "to": "end"}
            ]
        }))
        .expect("repeated Rezka lifecycle map");
        let queue_states = BTreeMap::from([
            (
                "apparatus:catalog:press-001".to_string(),
                BTreeMap::from([(order_id.to_string(), "completed".to_string())]),
            ),
            (
                "apparatus:default:asset-010".to_string(),
                BTreeMap::from([(order_id.to_string(), "completed".to_string())]),
            ),
            (
                "apparatus:catalog:lam-001".to_string(),
                BTreeMap::from([(order_id.to_string(), "completed".to_string())]),
            ),
        ]);

        assert_eq!(
            derive_production_order_lifecycle_with_stage_events(
                &map,
                &queue_states,
                &completed_events(&["rezka_before_lamination"]),
            ),
            Some(ProductionOrderLifecycleStatus::InProgress)
        );
        assert_eq!(
            derive_production_order_lifecycle_with_stage_events(
                &map,
                &queue_states,
                &completed_events(&["rezka_before_lamination", "rezka_final"]),
            ),
            Some(ProductionOrderLifecycleStatus::ProductionCompleted)
        );
    }

    #[test]
    fn operational_projection_uses_queue_activity_and_terminal_lifecycle() {
        let order_id = "zakaz-operational-status";
        let queue_states = BTreeMap::from([
            (
                "apparatus:catalog:press-001".to_string(),
                BTreeMap::from([(order_id.to_string(), "completed".to_string())]),
            ),
            (
                "apparatus:catalog:lam-001".to_string(),
                BTreeMap::from([(order_id.to_string(), "paused".to_string())]),
            ),
        ]);

        assert_eq!(
            derive_production_order_operational_status(
                ProductionOrderLifecycleStatus::InProgress,
                &queue_states,
                order_id,
                0,
                false,
                0,
            ),
            ProductionOrderOperationalStatus::Paused
        );
        assert_eq!(
            derive_production_order_operational_status(
                ProductionOrderLifecycleStatus::InProgress,
                &queue_states,
                order_id,
                0,
                true,
                0,
            ),
            ProductionOrderOperationalStatus::InProgress
        );
        assert_eq!(
            derive_production_order_operational_status(
                ProductionOrderLifecycleStatus::ProductionCompleted,
                &queue_states,
                order_id,
                1,
                false,
                0,
            ),
            ProductionOrderOperationalStatus::CompletedWithIssue
        );
        let empty_states = BTreeMap::new();
        assert_eq!(
            derive_production_order_operational_status(
                ProductionOrderLifecycleStatus::Released,
                &empty_states,
                order_id,
                0,
                false,
                1,
            ),
            ProductionOrderOperationalStatus::WaitingNextStage
        );
    }
}
