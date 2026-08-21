use std::collections::BTreeMap;

use super::super::chain;
use super::super::queue_state;
use super::super::types::{ProductionMapDefinition, ProductionOrderLogEntry};

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

pub(in crate::core::production_map) fn latest_required_complete_event<'a>(
    logs: &'a [ProductionOrderLogEntry],
    required_apparatus: &[String],
) -> Option<&'a ProductionOrderLogEntry> {
    logs.iter()
        .filter(|entry| {
            entry.action == queue_state::ApparatusQueueAction::Complete
                && entry.to_state == queue_state::ApparatusQueueOrderState::Completed
                && required_apparatus.iter().any(|apparatus| {
                    super::super::types::apparatus_ids_match(&entry.apparatus, apparatus)
                })
        })
        .max_by_key(|entry| entry.created_at_unix)
}

#[cfg(test)]
mod tests {
    use super::required_apparatus_for_closed_order;
    use crate::core::production_map::ProductionMapDefinition;

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
}
