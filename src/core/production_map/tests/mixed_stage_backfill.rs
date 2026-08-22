use std::collections::BTreeMap;
use std::sync::Arc;

use super::fixtures::apparatus_stage_map;
use crate::core::production_map::*;

fn two_stage_map() -> ProductionMapDefinition {
    let mut map = apparatus_stage_map("zakaz-backfill-test", "Bosma");
    map.nodes.insert(
        2,
        ProductionMapNode {
            id: "second".to_string(),
            kind: ProductionMapNodeKind::Apparatus,
            title: "Laminatsiya".to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: String::new(),
            alternative_group_label: String::new(),
            alternative_assigned_title: String::new(),
            rezka_kadr_count: None,
            rezka_label_length: None,
            x: 0.0,
            y: 264.0,
        },
    );
    map.edges = vec![
        ProductionMapEdge {
            from: "start".to_string(),
            to: "apparatus".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "apparatus".to_string(),
            to: "second".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "second".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];
    map
}

fn manifest(key: &str) -> MixedStageBackfillManifest {
    MixedStageBackfillManifest {
        version: 1,
        source: "pre_go_live_cutover_test".to_string(),
        records: vec![MixedStageBackfillRecord {
            idempotency_key: key.to_string(),
            order_id: "zakaz-backfill-test".to_string(),
            source_apparatus: "Bosma".to_string(),
            next_apparatus: "Laminatsiya".to_string(),
            current_location: "WIP rack A-03".to_string(),
            source_ref: "legacy sheet row 1".to_string(),
            started_at_unix: 1_700_000_000,
            completed_at_unix: 1_700_000_100,
            observed_at_unix: 1_700_000_200,
            produced_qty: 12_000.0,
            uom: "m".to_string(),
            label_item_code: "PRODUCT".to_string(),
            label_item_name: "Product".to_string(),
            executor_name: String::new(),
            worker_role: String::new(),
            worker_ref: String::new(),
            worker_display_name: String::new(),
            gross_qty: Some(12_100.0),
            return_ink_kg: Some(1.0),
            lamination_print_leftover_rolls: None,
            lamination_film_leftover_rolls: None,
            rezka_bosma_waste: None,
            rezka_lamination_waste: None,
            rezka_edge_waste: None,
            total_waste: Some(100.0),
            finished_goods_kg: None,
            bobina_kg: None,
            finished_goods_meter: Some(12_000.0),
            diameter: None,
            description: "measured historical WIP".to_string(),
        }],
    }
}

async fn seeded_service() -> (
    Arc<MemoryProductionMapStore>,
    ProductionMapService,
    BTreeMap<String, BTreeMap<String, String>>,
    BTreeMap<String, Vec<String>>,
    OrderControlRecord,
) {
    let store = Arc::new(MemoryProductionMapStore::new());
    store.put_map(two_stage_map()).await.expect("map");
    store
        .put_apparatus_sequence("Bosma", vec!["zakaz-backfill-test".to_string()])
        .await
        .expect("source sequence");
    store
        .put_apparatus_sequence("Laminatsiya", vec!["zakaz-backfill-test".to_string()])
        .await
        .expect("next sequence");
    store
        .put_apparatus_queue_states(
            "Bosma",
            BTreeMap::from([("zakaz-backfill-test".to_string(), "frozen".to_string())]),
        )
        .await
        .expect("source state");
    store
        .put_apparatus_queue_states(
            "Laminatsiya",
            BTreeMap::from([("zakaz-backfill-test".to_string(), "paused".to_string())]),
        )
        .await
        .expect("next state");
    let control = OrderControlRecord {
        order_id: "zakaz-backfill-test".to_string(),
        state: OrderControlState::Frozen,
        actor: QueueActionActor {
            role: "admin".to_string(),
            ref_: "admin-test".to_string(),
            display_name: "Admin test".to_string(),
        },
        requested_at_unix: 1_700_000_150,
        frozen_at_unix: Some(1_700_000_160),
        freeze_request: None,
    };
    store
        .put_order_control_state(control.clone())
        .await
        .expect("control");
    let before_states = store.apparatus_queue_states().await.expect("states");
    let before_sequences = store.apparatus_sequences().await.expect("sequences");
    (
        store.clone(),
        ProductionMapService::new(store),
        before_states,
        before_sequences,
        control,
    )
}

#[tokio::test]
async fn mixed_stage_backfill_preserves_queue_sequence_and_frozen_state() {
    let (store, service, before_states, before_sequences, before_control) = seeded_service().await;
    let plan = service
        .plan_mixed_stage_backfill(&manifest("stable-row-1"))
        .await
        .expect("plan");
    assert_eq!(plan.rows.len(), 1);
    assert_eq!(plan.rows[0].status, MixedStageBackfillPlanStatus::New);

    let report = service
        .apply_mixed_stage_backfill(&plan)
        .await
        .expect("apply");
    assert_eq!(report.applied, 1);
    assert_eq!(report.already_present, 0);
    assert_eq!(
        store.apparatus_queue_states().await.expect("states"),
        before_states
    );
    assert_eq!(
        store.apparatus_sequences().await.expect("sequences"),
        before_sequences
    );
    assert_eq!(
        store
            .order_control_states()
            .await
            .expect("controls")
            .get("zakaz-backfill-test"),
        Some(&before_control)
    );

    let batches = store
        .progress_batches_for_order("zakaz-backfill-test")
        .await
        .expect("batches");
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].wip_status, OrderProgressBatchWipStatus::Waiting);
    assert_eq!(batches[0].produced_qty, 12_000.0);
    assert_eq!(batches[0].total_waste, Some(100.0));
    assert_eq!(batches[0].finished_goods_meter, Some(12_000.0));
    assert_eq!(batches[0].current_apparatus, "Bosma");
    assert_eq!(batches[0].next_apparatus, "Laminatsiya");
    assert!(
        batches[0]
            .payload_json
            .get("historical_backfill")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    );
    assert_eq!(
        store
            .order_run_sessions_for_order("zakaz-backfill-test")
            .await
            .expect("sessions")
            .len(),
        1
    );
    assert!(
        store
            .queue_action_logs_for_orders(&["zakaz-backfill-test".to_string()])
            .await
            .expect("queue logs")
            .is_empty()
    );
}

#[tokio::test]
async fn mixed_stage_backfill_is_idempotent_and_rejects_changed_measurements() {
    let (store, service, _, _, _) = seeded_service().await;
    let first = service
        .plan_mixed_stage_backfill(&manifest("stable-row-2"))
        .await
        .expect("first plan");
    service
        .apply_mixed_stage_backfill(&first)
        .await
        .expect("first apply");

    let second = service
        .plan_mixed_stage_backfill(&manifest("stable-row-2"))
        .await
        .expect("second plan");
    assert_eq!(
        second.rows[0].status,
        MixedStageBackfillPlanStatus::AlreadyPresent
    );
    let second_report = service
        .apply_mixed_stage_backfill(&second)
        .await
        .expect("second apply");
    assert_eq!(second_report.applied, 0);
    assert_eq!(second_report.already_present, 1);
    assert_eq!(
        store
            .progress_batches_for_order("zakaz-backfill-test")
            .await
            .expect("batches")
            .len(),
        1
    );

    let mut changed = manifest("stable-row-2");
    changed.records[0].produced_qty = 12_001.0;
    assert!(matches!(
        service.plan_mixed_stage_backfill(&changed).await,
        Err(ProductionMapError::MixedStageBackfillConflict(_))
    ));
}

#[tokio::test]
async fn mixed_stage_backfill_rejects_missing_measurements_and_wrong_route() {
    let (_, service, _, _, _) = seeded_service().await;
    let mut missing_meter = manifest("invalid-row-1");
    missing_meter.records[0].finished_goods_meter = None;
    assert!(matches!(
        service.plan_mixed_stage_backfill(&missing_meter).await,
        Err(ProductionMapError::MixedStageBackfillInput(_))
    ));

    let mut wrong_route = manifest("invalid-row-2");
    wrong_route.records[0].next_apparatus = "Rezka".to_string();
    assert!(matches!(
        service.plan_mixed_stage_backfill(&wrong_route).await,
        Err(ProductionMapError::MixedStageBackfillInput(_))
    ));
}
