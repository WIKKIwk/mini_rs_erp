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
async fn downstream_start_requires_previous_stage_progress_qr() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-downstream-1".to_string(),
        display_name: "Worker Downstream".to_string(),
    };
    let order_id = "zakaz-downstream-1";
    let first = "Bosma aparat";
    let second = "Laminatsiya mashinasi";
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");

    service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("first start");

    let second_without_qr = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await;
    assert_eq!(
        second_without_qr,
        Err(ProductionMapError::ProgressQrRequired)
    );

    let paused = service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(18.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("first pause");
    let previous_batch = paused.progress_batch.expect("previous batch");

    let second_started = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                qr_payload: previous_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second start with previous qr");
    assert_eq!(
        second_started.states.get(order_id),
        Some(&"in_progress".to_string())
    );
    let event = second_started.progress_event.expect("start event");
    assert_eq!(
        event.payload_json["input_progress_batch_id"],
        previous_batch.batch_id
    );
    assert_eq!(event.payload_json["input_progress_apparatus"], first);
}

#[tokio::test]
async fn downstream_start_accepts_previous_stage_qr_after_resume() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-downstream-resume".to_string(),
        display_name: "Worker Downstream Resume".to_string(),
    };
    let order_id = "zakaz-downstream-resume";
    let first = "Bosma aparat";
    let second = "Laminatsiya mashinasi";
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");

    service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("first start");
    let paused = service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(12.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("first pause");
    let qr_payload = paused
        .progress_batch
        .as_ref()
        .expect("pause batch")
        .qr_payload
        .clone();
    service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Resume,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("first resume");

    let second_started = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                qr_payload,
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second start with resumed previous qr");
    assert_eq!(
        second_started.states.get(order_id),
        Some(&"in_progress".to_string())
    );
}

#[tokio::test]
async fn downstream_start_with_previous_qr_can_skip_pending_sequence_head() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-downstream-free".to_string(),
        display_name: "Worker Downstream Free".to_string(),
    };
    let first = "Bosma aparat";
    let second = "Laminatsiya mashinasi";
    let waiting_order = "zakaz-downstream-waiting";
    let ready_order = "zakaz-downstream-ready";
    service
        .upsert_map(two_stage_map(waiting_order, first, second))
        .await
        .expect("waiting map");
    service
        .upsert_map(two_stage_map(ready_order, first, second))
        .await
        .expect("ready map");
    service
        .set_apparatus_sequence(
            second,
            vec![waiting_order.to_string(), ready_order.to_string()],
        )
        .await
        .expect("second sequence");

    service
        .apply_apparatus_queue_action_with_progress(
            first,
            ready_order,
            queue_state::ApparatusQueueAction::Start,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .expect("first start ready order");
    let paused = service
        .apply_apparatus_queue_action_with_progress(
            first,
            ready_order,
            queue_state::ApparatusQueueAction::Pause,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(9.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("first pause ready order");
    let qr_payload = paused
        .progress_batch
        .as_ref()
        .expect("pause batch")
        .qr_payload
        .clone();

    let second_started = service
        .apply_apparatus_queue_action_with_progress(
            second,
            ready_order,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                qr_payload,
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second start skips waiting order with previous qr");
    assert_eq!(
        second_started.states.get(ready_order),
        Some(&"in_progress".to_string())
    );
    assert_ne!(
        second_started.states.get(waiting_order),
        Some(&"in_progress".to_string())
    );
}

#[tokio::test]
async fn downstream_start_marks_previous_stage_batch_in_use() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-wip-in-use".to_string(),
        display_name: "Worker WIP In Use".to_string(),
    };
    let first = "Bosma aparat";
    let second = "Laminatsiya mashinasi";
    let order_id = "zakaz-wip-in-use";
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");
    let first_batch = pause_first_stage_batch(&service, order_id, first, &actor, 21.0)
        .await
        .expect("first batch");

    service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                qr_payload: first_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second start");

    let updated = service
        .progress_batch_for_qr("", &first_batch.qr_payload)
        .await
        .expect("updated first batch");
    assert_eq!(updated.payload_json["wip_status"], "in_use");
    assert_eq!(updated.payload_json["current_apparatus"], second);
    assert_eq!(updated.payload_json["used_by_order_id"], order_id);
}

#[tokio::test]
async fn wip_listing_backfills_missing_next_apparatus_from_map() {
    let store = std::sync::Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new(store.clone());
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-wip-next".to_string(),
        display_name: "Worker WIP Next".to_string(),
    };
    let first = "7 ta rangli pechat";
    let second = "Laminatsiya 1";
    let order_id = "zakaz-wip-next";
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");
    let mut batch = pause_first_stage_batch(&service, order_id, first, &actor, 21.0)
        .await
        .expect("first batch");
    batch.next_apparatus.clear();
    batch.payload_json["next_apparatus"] = serde_json::json!("");
    store
        .put_order_progress_batch(batch)
        .await
        .expect("legacy batch update");

    let batches = service
        .wip_progress_batches("", Some(OrderProgressBatchWipStatus::Waiting), order_id, 10)
        .await
        .expect("wip batches");

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].next_apparatus, second);
    assert_eq!(batches[0].payload_json["next_apparatus"], second);
}

#[tokio::test]
async fn downstream_output_processes_input_batch_and_links_new_wip_batch() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-wip-processed".to_string(),
        display_name: "Worker WIP Processed".to_string(),
    };
    let first = "Bosma aparat";
    let second = "Laminatsiya mashinasi";
    let order_id = "zakaz-wip-processed";
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");
    let first_batch = pause_first_stage_batch(&service, order_id, first, &actor, 21.0)
        .await
        .expect("first batch");
    service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: first_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second start");

    let completed = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                produced_qty: Some(18.0),
                uom: "kg".to_string(),
                lamination_film_leftover_rolls: Some(1.0),
                total_waste: Some(0.5),
                finished_goods_kg: Some(18.0),
                finished_goods_meter: Some(120.0),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("second complete");

    let input = service
        .progress_batch_for_qr("", &first_batch.qr_payload)
        .await
        .expect("processed first batch");
    assert_eq!(input.payload_json["wip_status"], "processed");
    assert_eq!(input.payload_json["processed_by_apparatus"], second);

    let output = completed.progress_batch.expect("second output batch");
    assert_eq!(output.payload_json["wip_status"], "waiting");
    assert_eq!(output.payload_json["parent_batch_id"], first_batch.batch_id);
    assert_eq!(output.payload_json["from_apparatus"], second);
}

#[tokio::test]
async fn downstream_start_rejects_mismatched_progress_batch_id_and_qr() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-downstream-mismatch".to_string(),
        display_name: "Worker Downstream Mismatch".to_string(),
    };
    let first = "Bosma aparat";
    let second = "Laminatsiya mashinasi";
    let first_order = "zakaz-downstream-match";
    let second_order = "zakaz-downstream-other";
    service
        .upsert_map(two_stage_map(first_order, first, second))
        .await
        .expect("first map");
    service
        .upsert_map(two_stage_map(second_order, first, second))
        .await
        .expect("second map");

    let first_batch = pause_first_stage_batch(&service, first_order, first, &actor, 11.0)
        .await
        .expect("first batch");
    service
        .apply_apparatus_queue_action_with_progress(
            first,
            first_order,
            queue_state::ApparatusQueueAction::Resume,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                qr_payload: first_batch.qr_payload.clone(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("first resume");
    service
        .apply_apparatus_queue_action_with_progress(
            first,
            first_order,
            queue_state::ApparatusQueueAction::Complete,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(11.0),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await
        .expect("first complete");
    let second_batch = pause_first_stage_batch(&service, second_order, first, &actor, 12.0)
        .await
        .expect("second batch");

    let rejected = service
        .apply_apparatus_queue_action_with_progress(
            second,
            first_order,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                progress_batch_id: first_batch.batch_id,
                qr_payload: second_batch.qr_payload,
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(rejected, Err(ProductionMapError::ProgressBatchNotFound));
}

#[tokio::test]
async fn downstream_start_requires_qr_payload_not_only_progress_batch_id() {
    let service = ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));
    let actor = QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-downstream-id-only".to_string(),
        display_name: "Worker Downstream Id Only".to_string(),
    };
    let first = "Bosma aparat";
    let second = "Laminatsiya mashinasi";
    let order_id = "zakaz-downstream-id-only";
    service
        .upsert_map(two_stage_map(order_id, first, second))
        .await
        .expect("map");
    let batch = pause_first_stage_batch(&service, order_id, first, &actor, 8.0)
        .await
        .expect("batch");

    let rejected = service
        .apply_apparatus_queue_action_with_progress(
            second,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[second.to_string()],
            actor,
            QueueProgressInput {
                progress_batch_id: batch.batch_id,
                ..QueueProgressInput::default()
            },
        )
        .await;
    assert_eq!(rejected, Err(ProductionMapError::ProgressQrRequired));
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

async fn pause_first_stage_batch(
    service: &ProductionMapService,
    order_id: &str,
    first: &str,
    actor: &QueueActionActor,
    qty: f64,
) -> Result<OrderProgressBatch, ProductionMapError> {
    service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await?;
    let paused = service
        .apply_apparatus_queue_action_with_progress(
            first,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[first.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(qty),
                uom: "kg".to_string(),
                ..QueueProgressInput::default()
            },
        )
        .await?;
    paused
        .progress_batch
        .ok_or(ProductionMapError::ProgressBatchNotFound)
}

fn two_stage_map(id: &str, first: &str, second: &str) -> ProductionMapDefinition {
    let mut map = apparatus_stage_map(id, first);
    map.nodes.insert(
        2,
        ProductionMapNode {
            id: "second".to_string(),
            kind: ProductionMapNodeKind::Apparatus,
            title: second.to_string(),
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
    if let Some(end) = map.nodes.iter_mut().find(|node| node.id == "end") {
        end.y = 396.0;
    }
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
