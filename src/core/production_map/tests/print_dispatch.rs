use std::{collections::BTreeMap, sync::Arc};

use crate::core::apparatus_standard::{
    ProcessTechnology,
    test_support::{TestApparatusSpec, runtime_configuration},
};
use crate::core::production_map::*;

const PRINT: [&str; 3] = ["apparatus:test:a", "apparatus:test:b", "apparatus:test:c"];
const LAM: [&str; 2] = ["apparatus:test:d", "apparatus:test:e"];
const CUT: [&str; 2] = ["apparatus:test:f", "apparatus:test:g"];
const ORDER: &str = "zakaz-print-dispatch";

fn fixture() -> (ProductionMapService, Arc<MemoryProductionMapStore>) {
    // Opaque IDs and misleading display names must not determine the operation.
    let mut configs = PRINT
        .iter()
        .map(|id| {
            runtime_configuration(TestApparatusSpec::print(
                id,
                "Laminatsiya display",
                ProcessTechnology::Rotogravure,
                Some(8),
            ))
        })
        .collect::<Vec<_>>();
    configs.extend(
        LAM.iter()
            .map(|id| runtime_configuration(TestApparatusSpec::laminate(id, "Bosma display"))),
    );
    configs.extend(
        CUT.iter()
            .map(|id| runtime_configuration(TestApparatusSpec::cut(id, "Bosma display"))),
    );
    let store = Arc::new(MemoryProductionMapStore::new());
    (
        ProductionMapService::new(
            store.clone(),
            Arc::new(TestCanonicalApparatusResolver::new(configs)),
        ),
        store,
    )
}

fn map() -> ProductionMapDefinition {
    let mut nodes = vec![serde_json::json!({"id":"start", "kind":"start", "title":"Start"})];
    for (group, ids) in [
        ("print", PRINT.as_slice()),
        ("lam", LAM.as_slice()),
        ("cut", CUT.as_slice()),
    ] {
        for id in ids {
            nodes.push(
                serde_json::json!({"id":id, "kind":"apparatus", "title":"Display only",
                "apparatus_id":id, "alternative_group_id":group, "rezka_kadr_count":1}),
            );
        }
    }
    nodes.push(serde_json::json!({"id":"end", "kind":"end", "title":"End"}));
    serde_json::from_value(serde_json::json!({
        "id":ORDER, "product_code":"DISPATCH", "title":"Dispatch", "roll_count":3, "width_mm":650,
        "nodes":nodes,
        // Representative edges are also used by automatically generated maps.
        "edges":[{"from":"start","to":PRINT[0]}, {"from":PRINT[0],"to":LAM[0]},
            {"from":LAM[0],"to":CUT[0]}, {"from":CUT[0],"to":"end"}]
    }))
    .unwrap()
}

fn assign(map: &mut ProductionMapDefinition, group: &str, apparatus: &str) {
    for node in &mut map.nodes {
        if node.alternative_group_id == group {
            node.alternative_assigned_apparatus_id = apparatus.into();
            node.alternative_assigned_title = if apparatus.is_empty() {
                ""
            } else {
                "Display only"
            }
            .into();
        }
    }
}

fn contains(orders: &BTreeMap<String, Vec<String>>, apparatus: &str) -> bool {
    orders
        .get(apparatus)
        .is_some_and(|ids| ids.iter().any(|id| id == ORDER))
}

async fn assert_visibility(service: &ProductionMapService, selected: Option<&str>) {
    let visible = service.visible_order_ids_by_apparatus().await.unwrap();
    let sequences = service.effective_apparatus_sequences().await.unwrap();
    let snapshot = service.live_snapshot_shared().await.unwrap();
    let controls = service.queue_action_controls().await.unwrap();
    for id in PRINT {
        let expected = selected == Some(id);
        assert_eq!(contains(&visible, id), expected, "visible {id}");
        assert_eq!(contains(&sequences, id), expected, "sequence {id}");
        assert_eq!(
            contains(&snapshot.visible_order_ids, id),
            expected,
            "snapshot {id}"
        );
        assert_eq!(
            contains(&snapshot.sequences, id),
            expected,
            "snapshot sequence {id}"
        );
        assert_eq!(
            controls
                .get(id)
                .is_some_and(|orders| orders.contains_key(ORDER)),
            expected,
            "controls {id}"
        );
        assert_eq!(
            snapshot
                .queue_action_controls
                .get(id)
                .is_some_and(|orders| orders.contains_key(ORDER)),
            expected
        );
    }
    for id in LAM.into_iter().chain(CUT) {
        assert!(contains(&visible, id), "cooperative visibility {id}");
        assert!(
            contains(&snapshot.visible_order_ids, id),
            "cooperative snapshot {id}"
        );
    }
}

#[tokio::test]
async fn print_dispatch_assignment_move_and_return_preserve_downstream_cooperation() {
    let (service, store) = fixture();
    service.upsert_map(map()).await.unwrap();
    // Old saved sequences must not make an unassigned candidate visible again.
    for id in PRINT {
        store
            .put_apparatus_sequence(id, vec![ORDER.into()])
            .await
            .unwrap();
    }
    assert_visibility(&service, None).await;

    // The Unassigned -> machine UI saves this existing group assignment.
    let mut assigned = map();
    assign(&mut assigned, "print", PRINT[1]);
    assign(&mut assigned, "lam", LAM[0]);
    assign(&mut assigned, "cut", CUT[0]);
    service.upsert_map(assigned).await.unwrap();
    assert_visibility(&service, Some(PRINT[1])).await;

    service
        .move_apparatus(ProductionMapMoveRequest {
            map_id: ORDER.into(),
            from_apparatus: PRINT[1].into(),
            to_apparatus: PRINT[2].into(),
        })
        .await
        .unwrap();
    assert_visibility(&service, Some(PRINT[2])).await;

    let mut returned = service.map(ORDER).await.unwrap().unwrap().map;
    assign(&mut returned, "print", "");
    service.upsert_map(returned).await.unwrap();
    assert_visibility(&service, None).await;
}

#[tokio::test]
async fn print_dispatch_blocks_unselected_start_and_allows_only_selected_start() {
    let (service, store) = fixture();
    service.upsert_map(map()).await.unwrap();
    for selected in [None, Some(PRINT[1])] {
        if let Some(id) = selected {
            let mut assigned = map();
            assign(&mut assigned, "print", id);
            service.upsert_map(assigned).await.unwrap();
        }
        for id in PRINT.into_iter().filter(|id| Some(*id) != selected) {
            store
                .put_apparatus_sequence(id, vec![ORDER.into()])
                .await
                .unwrap();
            let result = service
                .apply_apparatus_queue_action_with_progress(
                    id,
                    ORDER,
                    queue_state::ApparatusQueueAction::Start,
                    &[id.into()],
                    QueueActionActor::default(),
                    QueueProgressInput::default(),
                )
                .await;
            assert!(
                matches!(result, Err(ProductionMapError::QueueActionNotAllowed)),
                "{id}: {result:?}"
            );
            assert!(matches!(
                service.set_apparatus_sequence(id, vec![ORDER.into()]).await,
                Err(ProductionMapError::QueueSequenceApparatusMismatch(_))
            ));
        }
    }
    assert!(
        store
            .order_run_sessions_for_order(ORDER)
            .await
            .unwrap()
            .is_empty()
    );
    service
        .set_apparatus_sequence(PRINT[1], vec![ORDER.into()])
        .await
        .unwrap();
    let started = service
        .apply_apparatus_queue_action_with_progress(
            PRINT[1],
            ORDER,
            queue_state::ApparatusQueueAction::Start,
            &[PRINT[1].into()],
            QueueActionActor::default(),
            QueueProgressInput::default(),
        )
        .await
        .expect("selected print starts");
    assert_eq!(started.session.unwrap().apparatus, PRINT[1]);

    // A selected non-representative printer still feeds either laminator.
    let output = service
        .apply_apparatus_queue_action_with_progress(
            PRINT[1],
            ORDER,
            queue_state::ApparatusQueueAction::Pause,
            &[PRINT[1].into()],
            QueueActionActor::default(),
            QueueProgressInput {
                produced_qty: Some(20.0),
                uom: "kg".into(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    let downstream = service
        .apply_apparatus_queue_action_with_progress(
            LAM[1],
            ORDER,
            queue_state::ApparatusQueueAction::Start,
            &[LAM[1].into()],
            QueueActionActor::default(),
            QueueProgressInput {
                qr_payload: output.qr_payload,
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("another laminator can consume the selected printer output");
    assert_eq!(downstream.session.unwrap().apparatus, LAM[1]);
}

#[tokio::test]
async fn print_dispatch_does_not_hide_direct_print_orders_or_single_flexo() {
    let (service, _) = fixture();
    let mut direct = map();
    for node in &mut direct.nodes {
        if node.apparatus_id == PRINT[0] {
            node.alternative_group_id.clear();
        }
    }
    service.upsert_map(direct).await.unwrap();
    let visible = service.visible_order_ids_by_apparatus().await.unwrap();
    assert!(contains(&visible, PRINT[0]));

    let flexo = runtime_configuration(TestApparatusSpec::print(
        PRINT[0],
        "Opaque",
        ProcessTechnology::Flexographic,
        Some(8),
    ));
    let mut alternative = map();
    assert!(!apparatus::print_assignment_allows_order(
        &alternative,
        &flexo
    ));
    assign(&mut alternative, "print", PRINT[0]);
    assert!(apparatus::print_assignment_allows_order(
        &alternative,
        &flexo
    ));
    alternative
        .nodes
        .iter_mut()
        .find(|n| n.apparatus_id == PRINT[0])
        .unwrap()
        .alternative_group_id
        .clear();
    assert!(apparatus::print_assignment_allows_order(
        &alternative,
        &flexo
    ));
}
