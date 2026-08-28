use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::production_map::*;

use super::fixtures::apparatus_stage_map;

const LAMINATION_ID: &str = "apparatus:default:asset-007";
const REZKA_ID: &str = "apparatus:default:asset-010";

fn opening_input(order_id: &str, idempotency_key: &str) -> OpeningWipCreateInput {
    OpeningWipCreateInput {
        idempotency_key: idempotency_key.to_string(),
        order_id: order_id.to_string(),
        entry_apparatus: LAMINATION_ID.to_string(),
        source_operation: "Bosma".to_string(),
        source_apparatus: String::new(),
        current_location: "Laminatsiya oldi".to_string(),
        note: "Day-0 sanoq".to_string(),
        batches: vec![
            OpeningWipBatchInput {
                quantity_basis: OpeningWipQuantityBasis::Unknown,
                quantity: None,
                uom: String::new(),
            },
            OpeningWipBatchInput {
                quantity_basis: OpeningWipQuantityBasis::Measured,
                quantity: Some(125.5),
                uom: "kg".to_string(),
            },
        ],
    }
}

fn admin_actor() -> QueueActionActor {
    QueueActionActor {
        role: "admin".to_string(),
        ref_: "admin:opening-wip".to_string(),
        display_name: "Opening WIP admin".to_string(),
    }
}

fn worker_actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker:opening-wip".to_string(),
        display_name: "Opening WIP worker".to_string(),
    }
}

fn lamination_complete_input() -> QueueProgressInput {
    QueueProgressInput {
        produced_qty: Some(10.0),
        uom: "kg".to_string(),
        lamination_print_leftover_rolls: Some(0.0),
        lamination_film_leftover_rolls: Some(0.0),
        total_waste: Some(0.0),
        finished_goods_kg: Some(10.0),
        finished_goods_meter: Some(100.0),
        ..QueueProgressInput::default()
    }
}

#[tokio::test]
async fn opening_wip_creates_unique_batches_and_replays_idempotently() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new_for_test(store.clone());
    let order_id = "zakaz-opening-wip-1";
    service
        .upsert_map(apparatus_stage_map(order_id, LAMINATION_ID))
        .await
        .expect("opening order map");

    let input = opening_input(order_id, "opening-wip-request-1");
    let created = service
        .create_opening_wip(input.clone(), admin_actor())
        .await
        .expect("opening WIP created");
    assert_eq!(created.intake.order_id, order_id);
    assert_eq!(created.intake.entry_apparatus, LAMINATION_ID);
    assert_eq!(created.intake.history_status, "unavailable_before_cutover");
    assert_eq!(created.batches.len(), 2);
    assert_ne!(created.batches[0].batch_id, created.batches[1].batch_id);
    assert_ne!(created.batches[0].qr_payload, created.batches[1].qr_payload);
    assert_eq!(created.batches[0].wip_status, OpeningWipBatchStatus::Waiting);
    assert_eq!(created.batches[0].quantity, None);
    assert_eq!(created.batches[1].quantity, Some(125.5));

    let replayed = service
        .create_opening_wip(input, admin_actor())
        .await
        .expect("idempotent replay");
    assert_eq!(replayed, created);

    let fetched = service
        .opening_wip_batch("", &created.batches[1].qr_payload)
        .await
        .expect("opening WIP lookup");
    assert_eq!(fetched.intake.intake_id, created.intake.intake_id);
    assert_eq!(fetched.batch.batch_id, created.batches[1].batch_id);
}

#[tokio::test]
async fn opening_wip_rejects_non_entry_apparatus_and_started_order() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new_for_test(store.clone());
    let order_id = "zakaz-opening-wip-2";
    service
        .upsert_map(apparatus_stage_map(order_id, LAMINATION_ID))
        .await
        .expect("opening order map");

    let mut wrong_entry = opening_input(order_id, "opening-wip-request-wrong-entry");
    wrong_entry.entry_apparatus = REZKA_ID.to_string();
    assert_eq!(
        service.create_opening_wip(wrong_entry, admin_actor()).await,
        Err(ProductionMapError::OpeningWipEntryMismatch)
    );

    ProductionMapStorePort::put_apparatus_queue_states(
        store.as_ref(),
        LAMINATION_ID,
        BTreeMap::from([(order_id.to_string(), "in_progress".to_string())]),
    )
    .await
    .expect("started queue state");
    assert_eq!(
        service
            .create_opening_wip(
                opening_input(order_id, "opening-wip-request-started"),
                admin_actor(),
            )
            .await,
        Err(ProductionMapError::OpeningWipOrderAlreadyStarted)
    );
}

#[tokio::test]
async fn opening_wip_idempotency_conflict_fails_closed() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new_for_test(store);
    let order_id = "zakaz-opening-wip-3";
    service
        .upsert_map(apparatus_stage_map(order_id, LAMINATION_ID))
        .await
        .expect("opening order map");
    service
        .create_opening_wip(
            opening_input(order_id, "opening-wip-request-conflict"),
            admin_actor(),
        )
        .await
        .expect("initial opening WIP");

    let mut conflicting = opening_input(order_id, "opening-wip-request-conflict");
    conflicting.current_location = "Boshqa joy".to_string();
    assert_eq!(
        service.create_opening_wip(conflicting, admin_actor()).await,
        Err(ProductionMapError::OpeningWipIdempotencyConflict)
    );
}

#[tokio::test]
async fn opening_wip_batches_drive_entry_stage_until_every_roll_is_processed() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new_for_test(store.clone());
    let order_id = "zakaz-opening-wip-lifecycle";
    service
        .upsert_map(apparatus_stage_map(order_id, LAMINATION_ID))
        .await
        .expect("opening order map");
    let opening = service
        .create_opening_wip(
            opening_input(order_id, "opening-wip-lifecycle-request"),
            admin_actor(),
        )
        .await
        .expect("opening WIP created");
    let assigned = [LAMINATION_ID.to_string()];

    let controls = service
        .queue_action_controls()
        .await
        .expect("opening WIP controls");
    let control = controls
        .get(LAMINATION_ID)
        .and_then(|orders| orders.get(order_id))
        .expect("lamination control");
    assert_eq!(
        control.interaction.opening_wip_mode,
        ApparatusQueuePreviousWipMode::ScanRequired
    );
    assert!(
        control
            .allowed_actions
            .contains(&queue_state::ApparatusQueueAction::Start)
    );

    assert_eq!(
        service
            .apply_apparatus_queue_action_with_progress(
                LAMINATION_ID,
                order_id,
                queue_state::ApparatusQueueAction::Start,
                &assigned,
                worker_actor(),
                QueueProgressInput::default(),
            )
            .await,
        Err(ProductionMapError::ProgressQrRequired)
    );

    for (index, source) in opening.batches.iter().enumerate() {
        service
            .apply_apparatus_queue_action_with_progress(
                LAMINATION_ID,
                order_id,
                queue_state::ApparatusQueueAction::Start,
                &assigned,
                worker_actor(),
                QueueProgressInput {
                    qr_payload: source.qr_payload.clone(),
                    ..QueueProgressInput::default()
                },
            )
            .await
            .expect("entry stage starts opening WIP");
        let in_use = service
            .opening_wip_batch(&source.batch_id, "")
            .await
            .expect("in-use opening WIP");
        assert_eq!(in_use.batch.wip_status, OpeningWipBatchStatus::InUse);
        assert_eq!(in_use.batch.used_by_apparatus, LAMINATION_ID);

        service
            .apply_apparatus_queue_action_with_progress(
                LAMINATION_ID,
                order_id,
                queue_state::ApparatusQueueAction::Complete,
                &assigned,
                worker_actor(),
                lamination_complete_input(),
            )
            .await
            .expect("entry stage completes opening WIP roll");
        let processed = service
            .opening_wip_batch(&source.batch_id, "")
            .await
            .expect("processed opening WIP");
        assert_eq!(
            processed.batch.wip_status,
            OpeningWipBatchStatus::Processed
        );

        let states = service.apparatus_queue_states().await.expect("queue states");
        let state = states
            .get(LAMINATION_ID)
            .and_then(|orders| orders.get(order_id))
            .map(String::as_str);
        if index + 1 < opening.batches.len() {
            assert_eq!(state, Some("pending"));
        } else {
            assert_eq!(state, Some("completed"));
        }
    }
}
