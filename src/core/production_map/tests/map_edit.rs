use std::collections::BTreeMap;

use crate::core::production_map::*;
use crate::core::{
    apparatus_standard::{
        ProcessTechnology,
        test_support::{TestApparatusSpec, runtime_configuration},
    },
    production_map::TestCanonicalApparatusResolver,
};

use super::fixtures::{
    canonical_apparatus_stage_map, canonical_two_stage_map, service_with_default_apparatus,
};

const PECHAT_7_ID: &str = "apparatus:default:bosma_7";
const PECHAT_8_ID: &str = "apparatus:default:bosma_8";
const FLEXO_ID: &str = "apparatus:default:asset-005";
const LAMINATION_ID: &str = "apparatus:default:asset-007";
const REZKA_ID: &str = "apparatus:default:asset-010";

fn service_with_flexo_limits() -> ProductionMapService {
    let mut flexo = TestApparatusSpec::print(
        FLEXO_ID,
        "Flexo pechat",
        ProcessTechnology::Flexographic,
        Some(8),
    );
    flexo.min_web_width_mm = Some(400);
    flexo.max_web_width_mm = Some(800);
    ProductionMapService::new(
        std::sync::Arc::new(MemoryProductionMapStore::new()),
        std::sync::Arc::new(TestCanonicalApparatusResolver::new([
            runtime_configuration(flexo),
        ])),
    )
}

#[tokio::test]
async fn flexo_map_saves_like_standard_print_regardless_of_profile_limits() {
    let service = service_with_flexo_limits();
    for (id, width_mm, roll_count) in [
        ("zakaz-flexo-min", 400.0, 1),
        ("zakaz-flexo-max", 800.0, 8),
        ("zakaz-flexo-narrow", 399.0, 8),
        ("zakaz-flexo-wide", 801.0, 8),
        ("zakaz-flexo-rolls", 650.0, 9),
    ] {
        let mut map = canonical_apparatus_stage_map(id, FLEXO_ID, "Flexo pechat");
        map.width_mm = Some(width_mm);
        map.roll_count = Some(roll_count);
        service
            .upsert_map(map)
            .await
            .expect("Flexo map must save like a standard print map");
    }
}

#[tokio::test]
async fn started_stage_is_immutable_but_future_stage_can_be_replaced() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = service_with_default_apparatus(store.clone()).await;
    let order_id = "zakaz-map-edit-locked";
    let first = PECHAT_7_ID;
    let original = canonical_two_stage_map(
        order_id,
        PECHAT_7_ID,
        "7 ta rangli bosma aparat",
        LAMINATION_ID,
        "Laminatsiya",
    );
    service
        .upsert_map(original.clone())
        .await
        .expect("initial map");
    store
        .put_apparatus_queue_states(
            first,
            BTreeMap::from([(order_id.to_string(), "in_progress".to_string())]),
        )
        .await
        .expect("queue state");

    let mut moved_started_stage = original.clone();
    moved_started_stage.nodes[1].x = 400.0;
    assert_eq!(
        service.upsert_map(moved_started_stage).await,
        Err(ProductionMapError::StartedProductionMapStageLocked)
    );

    let mut changed_started_route = original.clone();
    changed_started_route
        .edges
        .retain(|edge| !(edge.from == "start" && edge.to == "apparatus"));
    assert_eq!(
        service.upsert_map(changed_started_route).await,
        Err(ProductionMapError::StartedProductionMapStageLocked)
    );

    let mut replaced_future_stage = original.clone();
    replaced_future_stage.nodes[2].title = "Yangi laminatsiya aparat".to_string();
    service
        .upsert_map(replaced_future_stage.clone())
        .await
        .expect("future stage replacement");

    replaced_future_stage
        .nodes
        .retain(|node| node.id != "second");
    replaced_future_stage
        .edges
        .retain(|edge| edge.from != "second" && edge.to != "second");
    replaced_future_stage.edges.push(ProductionMapEdge {
        from: "apparatus".to_string(),
        to: "end".to_string(),
        branch: String::new(),
    });
    service
        .upsert_map(replaced_future_stage)
        .await
        .expect("future route removal");
}

#[tokio::test]
async fn completed_session_locks_stage_even_without_queue_state() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = service_with_default_apparatus(store.clone()).await;
    let order_id = "zakaz-map-edit-history";
    let apparatus = REZKA_ID;
    let original = canonical_apparatus_stage_map(order_id, apparatus, "Rezka");
    service
        .upsert_map(original.clone())
        .await
        .expect("initial map");
    store
        .put_order_run_session(OrderRunSession {
            session_id: "session-map-edit-history".to_string(),
            apparatus: apparatus.to_string(),
            order_id: order_id.to_string(),
            stage_node_id: "apparatus".to_string(),
            status: OrderRunStatus::Completed,
            worker_role: "aparatchi".to_string(),
            worker_ref: "worker-map-edit".to_string(),
            worker_display_name: "Map Edit Worker".to_string(),
            started_at_unix: 1,
            updated_at_unix: 2,
            payload_json: serde_json::json!({}),
        })
        .await
        .expect("completed session");

    let mut changed = original;
    changed.nodes[1].title = "Boshqa rezka".to_string();
    assert_eq!(
        service.upsert_map(changed).await,
        Err(ProductionMapError::StartedProductionMapStageLocked)
    );
}

#[tokio::test]
async fn pending_apparatus_move_still_uses_the_guarded_map_save() {
    let service =
        service_with_default_apparatus(std::sync::Arc::new(MemoryProductionMapStore::new())).await;
    let order_id = "zakaz-map-edit-pending-move";
    service
        .upsert_map(canonical_apparatus_stage_map(
            order_id,
            PECHAT_7_ID,
            "7 ta rangli bosma aparat",
        ))
        .await
        .expect("initial map");

    let moved = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        service.move_apparatus(ProductionMapMoveRequest {
            map_id: order_id.to_string(),
            from_apparatus: PECHAT_7_ID.to_string(),
            to_apparatus: PECHAT_8_ID.to_string(),
        }),
    )
    .await
    .expect("move must not deadlock")
    .expect("pending move");

    assert_eq!(moved.map.nodes[1].apparatus_id, PECHAT_8_ID);
}

#[tokio::test]
async fn pending_pechat_move_rejects_incompatible_order_dimensions() {
    let service =
        service_with_default_apparatus(std::sync::Arc::new(MemoryProductionMapStore::new())).await;
    let order_id = "zakaz-map-edit-pechat-capacity";
    let mut map = canonical_apparatus_stage_map(order_id, PECHAT_8_ID, "8 ta rangli bosma aparat");
    map.roll_count = Some(7);
    map.width_mm = Some(900.0);
    service.upsert_map(map).await.expect("initial map");

    let result = service
        .move_apparatus(ProductionMapMoveRequest {
            map_id: order_id.to_string(),
            from_apparatus: PECHAT_8_ID.to_string(),
            to_apparatus: PECHAT_7_ID.to_string(),
        })
        .await;

    assert_eq!(result, Err(ProductionMapError::MoveNotAllowed));
}

#[tokio::test]
async fn pending_unassigned_alternative_move_claims_target_apparatus() {
    let service =
        service_with_default_apparatus(std::sync::Arc::new(MemoryProductionMapStore::new())).await;
    let order_id = "zakaz-map-edit-unassigned-alternative";
    service
        .upsert_map(unassigned_alternative_map(order_id))
        .await
        .expect("initial alternative map");

    let moved = service
        .move_apparatus(ProductionMapMoveRequest {
            map_id: order_id.to_string(),
            from_apparatus: PECHAT_7_ID.to_string(),
            to_apparatus: PECHAT_8_ID.to_string(),
        })
        .await
        .expect("unassigned alternative move");

    assert_eq!(
        moved
            .map
            .nodes
            .iter()
            .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
            .map(|node| node.title.as_str())
            .collect::<Vec<_>>(),
        vec!["7 ta rangli bosma aparat", "8 ta rangli bosma aparat"]
    );
    assert_eq!(
        moved
            .map
            .nodes
            .iter()
            .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
            .map(|node| node.alternative_assigned_title.as_str())
            .collect::<Vec<_>>(),
        vec!["8 ta rangli bosma aparat", "8 ta rangli bosma aparat"]
    );
}

fn unassigned_alternative_map(id: &str) -> ProductionMapDefinition {
    let mut map = canonical_apparatus_stage_map(id, PECHAT_7_ID, "7 ta rangli bosma aparat");
    for node in map
        .nodes
        .iter_mut()
        .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
    {
        node.alternative_group_id = "alt-pechat".to_string();
        node.alternative_group_label = "pechat".to_string();
        node.alternative_assigned_title.clear();
    }
    map.nodes.insert(
        2,
        ProductionMapNode {
            id: "apparatus-8".to_string(),
            kind: ProductionMapNodeKind::Apparatus,
            title: "8 ta rangli bosma aparat".to_string(),
            apparatus_id: PECHAT_8_ID.to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: "alt-pechat".to_string(),
            alternative_group_label: "pechat".to_string(),
            alternative_assigned_title: String::new(),
            alternative_assigned_apparatus_id: String::new(),
            rezka_kadr_count: None,
            rezka_frame_groups: Vec::new(),
            rezka_label_length: None,
            x: 0.0,
            y: 264.0,
        },
    );
    map.nodes
        .iter_mut()
        .find(|node| node.kind == ProductionMapNodeKind::End)
        .expect("end node")
        .y = 396.0;
    map.edges = vec![
        ProductionMapEdge {
            from: "start".to_string(),
            to: "apparatus".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "start".to_string(),
            to: "apparatus-8".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "apparatus".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "apparatus-8".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];
    map
}
