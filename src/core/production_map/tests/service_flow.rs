use std::collections::BTreeMap;

use crate::core::production_map::*;

use super::fixtures::{apparatus_stage_map, sample_map};

#[tokio::test]
async fn maps_skips_legacy_invalid_map_without_failing_list() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let mut valid = sample_map();
    valid.id = "valid-map".to_string();
    let mut invalid = sample_map();
    invalid.id = "invalid-laminatsiya".to_string();
    invalid.width_mm = Some(1070.0);
    invalid.nodes[2].title = "Laminatsiya".to_string();

    store.put_map(valid).await.expect("valid insert");
    store.put_map(invalid).await.expect("invalid legacy insert");

    let service = ProductionMapService::new(store);
    let maps = service.maps().await.expect("list");
    assert_eq!(maps.len(), 1);
    assert_eq!(maps[0].map.id, "valid-map");
    assert_eq!(
        service.map("invalid-laminatsiya").await,
        Err(ProductionMapError::LaminatsiyaRubberTooLarge)
    );
}

#[tokio::test]
async fn free_pick_policy_allows_ready_order_outside_sequence_head() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store);
    let actor = QueueActionActor {
        role: "admin".to_string(),
        ref_: "admin".to_string(),
        display_name: "Admin".to_string(),
    };
    let first = apparatus_stage_map("zakaz-1", "Rezka apparat");
    let second = apparatus_stage_map("zakaz-2", "Rezka apparat");
    service.upsert_map(first).await.expect("first map");
    service.upsert_map(second).await.expect("second map");
    service
        .set_apparatus_sequence(
            "Rezka apparat",
            vec!["zakaz-1".to_string(), "zakaz-2".to_string()],
        )
        .await
        .expect("sequence");

    let strict_result = service
        .apply_apparatus_queue_action(
            "Rezka apparat",
            "zakaz-2",
            queue_state::ApparatusQueueAction::Start,
            &["Rezka apparat".to_string()],
            actor.clone(),
        )
        .await;
    assert_eq!(
        strict_result,
        Err(ProductionMapError::QueueActionNotAllowed)
    );

    service
        .set_apparatus_queue_policy("Rezka apparat", ApparatusQueuePolicy::FreePick, &actor)
        .await
        .expect("free pick policy");
    let states = service
        .apply_apparatus_queue_action(
            "Rezka apparat",
            "zakaz-2",
            queue_state::ApparatusQueueAction::Start,
            &["Rezka apparat".to_string()],
            actor,
        )
        .await
        .expect("start second");
    assert_eq!(states.get("zakaz-2"), Some(&"in_progress".to_string()));
}

#[tokio::test]
async fn pechat_queue_policy_is_always_locked_strict() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let actor = QueueActionActor {
        role: "admin".to_string(),
        ref_: "admin".to_string(),
        display_name: "Admin".to_string(),
    };
    let result = service
        .set_apparatus_queue_policy("7 ta rangli pechat", ApparatusQueuePolicy::FreePick, &actor)
        .await;
    assert_eq!(result, Err(ProductionMapError::ApparatusQueuePolicyLocked));
}

#[tokio::test]
async fn raw_material_assignment_requires_exact_scan_before_start() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-1".to_string(),
        display_name: "Worker 1".to_string(),
    };
    service
        .upsert_map(apparatus_stage_map("zakaz-raw-1", "7 ta rangli pechat - A"))
        .await
        .expect("map");
    service
        .set_apparatus_material_rule(ApparatusMaterialRuleUpsert {
            apparatus: "7 ta rangli pechat - A".to_string(),
            requires_material: true,
            item_groups: vec!["Kraska".to_string()],
        })
        .await
        .expect("material rule");
    let missing_assignment = service
        .apply_apparatus_queue_action_with_material_scan(
            "7 ta rangli pechat - A",
            "zakaz-raw-1",
            queue_state::ApparatusQueueAction::Start,
            &["7 ta rangli pechat - A".to_string()],
            actor.clone(),
            "",
        )
        .await;
    assert_eq!(
        missing_assignment,
        Err(ProductionMapError::RawMaterialAssignmentNotFound)
    );
    let assigned = service
        .assign_raw_material_to_order(
            RawMaterialAssignmentInput {
                order_id: "zakaz-raw-1".to_string(),
                barcode: "30AA".to_string(),
                item_code: "INK-BLACK".to_string(),
                item_name: "Black ink".to_string(),
                item_group: "Kraska".to_string(),
                item_group_path: Vec::new(),
            },
            &actor,
        )
        .await
        .expect("assign material");
    assert_eq!(assigned.apparatus, "7 ta rangli pechat - A");
    let second_assigned = service
        .assign_raw_material_to_order(
            RawMaterialAssignmentInput {
                order_id: "zakaz-raw-1".to_string(),
                barcode: "30CC".to_string(),
                item_code: "INK-WHITE".to_string(),
                item_name: "White ink".to_string(),
                item_group: "Kraska".to_string(),
                item_group_path: Vec::new(),
            },
            &actor,
        )
        .await
        .expect("assign second material");
    assert_eq!(second_assigned.apparatus, "7 ta rangli pechat - A");

    service
        .upsert_map(apparatus_stage_map("zakaz-raw-2", "7 ta rangli pechat - A"))
        .await
        .expect("second map");
    let duplicate = service
        .assign_raw_material_to_order(
            RawMaterialAssignmentInput {
                order_id: "zakaz-raw-2".to_string(),
                barcode: "30AA".to_string(),
                item_code: "INK-BLACK".to_string(),
                item_name: "Black ink".to_string(),
                item_group: "Kraska".to_string(),
                item_group_path: Vec::new(),
            },
            &actor,
        )
        .await;
    assert_eq!(
        duplicate,
        Err(ProductionMapError::RawMaterialAlreadyAssigned)
    );

    let not_assigned = service
        .apply_apparatus_queue_action_with_material_scan(
            "7 ta rangli pechat - A",
            "zakaz-raw-1",
            queue_state::ApparatusQueueAction::Start,
            &["Rezka apparat".to_string()],
            actor.clone(),
            "",
        )
        .await;
    assert_eq!(not_assigned, Err(ProductionMapError::ApparatusNotAssigned));

    let missing_scan = service
        .apply_apparatus_queue_action_with_material_scan(
            "7 ta rangli pechat - A",
            "zakaz-raw-1",
            queue_state::ApparatusQueueAction::Start,
            &["7 ta rangli pechat - A".to_string()],
            actor.clone(),
            "",
        )
        .await;
    assert_eq!(
        missing_scan,
        Err(ProductionMapError::RawMaterialScanRequired)
    );

    let wrong_scan = service
        .apply_apparatus_queue_action_with_material_scan(
            "7 ta rangli pechat - A",
            "zakaz-raw-1",
            queue_state::ApparatusQueueAction::Start,
            &["7 ta rangli pechat - A".to_string()],
            actor.clone(),
            "30BB",
        )
        .await;
    assert_eq!(wrong_scan, Err(ProductionMapError::RawMaterialMismatch));

    let partial_scan = service
        .apply_apparatus_queue_action_with_material_scan(
            "7 ta rangli pechat - A",
            "zakaz-raw-1",
            queue_state::ApparatusQueueAction::Start,
            &["7 ta rangli pechat - A".to_string()],
            actor.clone(),
            "30AA",
        )
        .await;
    assert_eq!(partial_scan, Err(ProductionMapError::RawMaterialMismatch));

    let states = service
        .apply_apparatus_queue_action_with_material_scan(
            "7 ta rangli pechat - A",
            "zakaz-raw-1",
            queue_state::ApparatusQueueAction::Start,
            &["7 ta rangli pechat - A".to_string()],
            actor,
            "30AA,30CC",
        )
        .await
        .expect("start with exact material");
    assert_eq!(states.get("zakaz-raw-1"), Some(&"in_progress".to_string()));
}

#[tokio::test]
async fn progress_pause_creates_qr_batch_and_resume_reopens_order() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-progress-1".to_string(),
        display_name: "Worker Progress".to_string(),
    };
    service
        .upsert_map(apparatus_stage_map(
            "zakaz-progress-1",
            "7 ta rangli pechat",
        ))
        .await
        .expect("map");

    let started = service
        .apply_apparatus_queue_action_with_progress(
            "7 ta rangli pechat",
            "zakaz-progress-1",
            queue_state::ApparatusQueueAction::Start,
            &["7 ta rangli pechat".to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("start");
    assert_eq!(
        started.states.get("zakaz-progress-1"),
        Some(&"in_progress".to_string())
    );
    assert!(started.session.is_some());
    assert!(started.progress_batch.is_none());

    let paused = service
        .apply_apparatus_queue_action_with_progress(
            "7 ta rangli pechat",
            "zakaz-progress-1",
            queue_state::ApparatusQueueAction::Pause,
            &["7 ta rangli pechat".to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(42.5),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("pause");
    assert_eq!(
        paused.states.get("zakaz-progress-1"),
        Some(&"paused".to_string())
    );
    let batch = paused.progress_batch.expect("pause batch");
    assert_eq!(batch.status, OrderProgressBatchStatus::Paused);
    assert_eq!(batch.produced_qty, 42.5);
    assert_eq!(batch.qr_payload.len(), 24);
    assert!(batch.qr_payload.starts_with("4001"));
    assert!(batch.qr_payload.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert!(batch.label_item_name.contains("pauza"));
    assert_eq!(batch.executor_name, "Worker Progress");

    let lookup = service
        .progress_batch_for_qr("", &batch.qr_payload)
        .await
        .expect("lookup");
    assert_eq!(lookup.batch_id, batch.batch_id);

    let resumed = service
        .apply_apparatus_queue_action_with_progress(
            "7 ta rangli pechat",
            "zakaz-progress-1",
            queue_state::ApparatusQueueAction::Resume,
            &["7 ta rangli pechat".to_string()],
            actor,
            QueueProgressInput {
                qr_payload: batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("resume");
    assert_eq!(
        resumed.states.get("zakaz-progress-1"),
        Some(&"in_progress".to_string())
    );
    assert_eq!(
        resumed.progress_batch.expect("resumed batch").status,
        OrderProgressBatchStatus::Resumed
    );
}

#[tokio::test]
async fn upsert_maps_batch_keeps_queue_state_and_sequence_cache() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store);
    service
        .set_apparatus_sequence(
            "7 ta rangli pechat - A",
            vec!["zakaz-111".to_string(), "zakaz-222".to_string()],
        )
        .await
        .expect("sequence");
    service
        .store
        .put_apparatus_queue_states(
            "7 ta rangli pechat - A",
            BTreeMap::from([("zakaz-111".to_string(), "completed".to_string())]),
        )
        .await
        .expect("queue state");
    let mut first = sample_map();
    first.id = "zakaz-111".to_string();
    first.order_number = "111".to_string();
    first.code = "111".to_string();
    let mut second = sample_map();
    second.id = "zakaz-222".to_string();
    second.order_number = "222".to_string();
    second.code = "222".to_string();

    let saved = service
        .upsert_maps_batch(vec![first, second])
        .await
        .expect("batch upsert");

    assert_eq!(saved.len(), 2);
    assert_eq!(service.maps().await.expect("maps").len(), 2);
    assert_eq!(
        service
            .apparatus_sequences()
            .await
            .expect("sequences")
            .get("7 ta rangli pechat - A"),
        Some(&vec!["zakaz-111".to_string(), "zakaz-222".to_string()])
    );
    assert_eq!(
        service
            .apparatus_queue_states()
            .await
            .expect("states")
            .get("7 ta rangli pechat - A")
            .and_then(|states| states.get("zakaz-111")),
        Some(&"completed".to_string())
    );
}
