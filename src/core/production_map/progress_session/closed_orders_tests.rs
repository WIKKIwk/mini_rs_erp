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
