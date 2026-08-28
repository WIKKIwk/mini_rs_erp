use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::production_map::*;

use super::fixtures::{apparatus_stage_map, canonical_two_stage_map};

const PECHAT_ID: &str = "apparatus:default:bosma_7";
const LAMINATION_ID: &str = "apparatus:default:asset-007";
const LAMINATION_2_ID: &str = "apparatus:default:asset-008";
const REZKA_ID: &str = "apparatus:default:asset-010";

fn opening_input(order_id: &str, idempotency_key: &str) -> OpeningWipCreateInput {
    OpeningWipCreateInput {
        idempotency_key: idempotency_key.to_string(),
        order_id: order_id.to_string(),
        entry_apparatus: LAMINATION_ID.to_string(),
        source_operation: "Bosma".to_string(),
        source_apparatus: String::new(),
        current_location: LAMINATION_ID.to_string(),
        note: "Day-0 sanoq".to_string(),
        batches: vec![
            OpeningWipBatchInput {
                quantity_basis: OpeningWipQuantityBasis::Estimated,
                finished_goods_meter: Some(100.0),
                finished_goods_kg: Some(12.0),
                bobina_kg: Some(1.0),
                diameter: None,
            },
            OpeningWipBatchInput {
                quantity_basis: OpeningWipQuantityBasis::Measured,
                finished_goods_meter: Some(125.5),
                finished_goods_kg: Some(14.0),
                bobina_kg: Some(1.1),
                diameter: None,
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
        lamination_print_leftover_rolls: Some(0.5),
        lamination_film_leftover_rolls: Some(0.5),
        total_waste: Some(0.5),
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
    assert_eq!(created.intake.current_location, LAMINATION_ID);
    assert_eq!(created.intake.resume_apparatus, LAMINATION_ID);
    assert_eq!(created.intake.resume_stage_node_id, "apparatus");
    assert_eq!(
        created.intake.source_operation,
        "unavailable_before_cutover"
    );
    assert!(created.intake.source_apparatus.is_empty());
    assert_eq!(created.intake.history_status, "unavailable_before_cutover");
    assert_eq!(created.batches.len(), 2);
    assert_ne!(created.batches[0].batch_id, created.batches[1].batch_id);
    assert_ne!(created.batches[0].qr_payload, created.batches[1].qr_payload);
    assert_eq!(
        created.batches[0].wip_status,
        OpeningWipBatchStatus::Waiting
    );
    assert_eq!(created.batches[0].quantity, Some(100.0));
    assert_eq!(created.batches[1].quantity, Some(125.5));
    assert_eq!(created.batches[1].finished_goods_kg, Some(14.0));
    assert_eq!(created.batches[1].bobina_kg, Some(1.1));

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
async fn opening_wip_requires_roll_passport_metrics_for_entry_operation() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new_for_test(store);
    let order_id = "zakaz-opening-wip-rezka";
    service
        .upsert_map(apparatus_stage_map(order_id, REZKA_ID))
        .await
        .expect("rezka opening order map");

    let mut missing_diameter = opening_input(order_id, "opening-wip-rezka-missing");
    missing_diameter.entry_apparatus = REZKA_ID.to_string();
    missing_diameter.current_location = REZKA_ID.to_string();
    assert_eq!(
        service
            .create_opening_wip(missing_diameter.clone(), admin_actor())
            .await,
        Err(ProductionMapError::OpeningWipInvalidInput)
    );

    for batch in &mut missing_diameter.batches {
        batch.diameter = Some(45.0);
    }
    let created = service
        .create_opening_wip(missing_diameter, admin_actor())
        .await
        .expect("rezka Opening WIP with diameter");
    assert!(
        created
            .batches
            .iter()
            .all(|batch| batch.diameter == Some(45.0))
    );
}

#[tokio::test]
async fn opening_wip_location_must_be_a_real_apparatus_in_the_order_map() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new_for_test(store);
    let order_id = "zakaz-opening-wip-location-map";
    service
        .upsert_map(canonical_two_stage_map(
            order_id,
            LAMINATION_ID,
            "Laminatsiya 1",
            REZKA_ID,
            "Rezka",
        ))
        .await
        .expect("two-stage opening order map");

    let mut at_rezka = opening_input(order_id, "opening-wip-location-rezka");
    at_rezka.current_location = REZKA_ID.to_string();
    let created = service
        .create_opening_wip(at_rezka, admin_actor())
        .await
        .expect("map apparatus is a valid current location");
    assert_eq!(created.intake.current_location, "Rezka");
    assert_eq!(created.intake.resume_apparatus, REZKA_ID);
    assert_eq!(created.intake.resume_stage_node_id, "second");

    let mut outside_map = opening_input(order_id, "opening-wip-location-outside");
    outside_map.current_location = "apparatus:default:asset-008".to_string();
    assert_eq!(
        service.create_opening_wip(outside_map, admin_actor()).await,
        Err(ProductionMapError::OpeningWipLocationMismatch)
    );

    let alternative_order_id = "zakaz-opening-wip-location-alternative";
    let mut alternative_map = canonical_two_stage_map(
        alternative_order_id,
        PECHAT_ID,
        "7 ta rangli bosma aparat",
        LAMINATION_ID,
        "Laminatsiya 1",
    );
    let mut lamination_2 = alternative_map
        .nodes
        .iter()
        .find(|node| node.id == "second")
        .expect("first laminatsiya alternative")
        .clone();
    lamination_2.id = "lamination-2".to_string();
    lamination_2.title = "Laminatsiya 2".to_string();
    lamination_2.apparatus_id = LAMINATION_2_ID.to_string();
    for node in alternative_map
        .nodes
        .iter_mut()
        .filter(|node| node.id == "second")
    {
        node.alternative_group_id = "alt-laminatsiya".to_string();
        node.alternative_group_label = "Laminatsiya".to_string();
    }
    lamination_2.alternative_group_id = "alt-laminatsiya".to_string();
    lamination_2.alternative_group_label = "Laminatsiya".to_string();
    alternative_map.nodes.insert(3, lamination_2);
    alternative_map.edges = vec![
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
            from: "apparatus".to_string(),
            to: "lamination-2".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "second".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "lamination-2".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];
    service
        .upsert_map(alternative_map)
        .await
        .expect("alternative opening order map");
    let mut at_second_alternative = opening_input(
        alternative_order_id,
        "opening-wip-location-second-alternative",
    );
    at_second_alternative.entry_apparatus = PECHAT_ID.to_string();
    at_second_alternative.current_location = LAMINATION_2_ID.to_string();
    let created = service
        .create_opening_wip(at_second_alternative, admin_actor())
        .await
        .expect("unassigned map alternative is a valid current location");
    assert_eq!(created.intake.current_location, "Laminatsiya 2");
    assert_eq!(created.intake.resume_apparatus, LAMINATION_2_ID);
    assert_eq!(created.intake.resume_stage_node_id, "lamination-2");
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
    conflicting.note = "Boshqa izoh".to_string();
    assert_eq!(
        service.create_opening_wip(conflicting, admin_actor()).await,
        Err(ProductionMapError::OpeningWipIdempotencyConflict)
    );
}

#[tokio::test]
async fn opening_wip_batches_resume_mid_map_stage_without_completing_source_stage() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new_for_test(store.clone());
    let order_id = "zakaz-opening-wip-lifecycle";
    service
        .upsert_map(canonical_two_stage_map(
            order_id,
            PECHAT_ID,
            "7 ta rangli bosma aparat",
            LAMINATION_ID,
            "Laminatsiya 1",
        ))
        .await
        .expect("opening order map");
    let mut input = opening_input(order_id, "opening-wip-lifecycle-request");
    input.entry_apparatus = PECHAT_ID.to_string();
    let opening = service
        .create_opening_wip(input, admin_actor())
        .await
        .expect("opening WIP created");
    assert_eq!(opening.intake.entry_apparatus, PECHAT_ID);
    assert_eq!(opening.intake.resume_apparatus, LAMINATION_ID);
    assert_eq!(opening.intake.resume_stage_node_id, "second");
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
    assert_eq!(
        control.interaction.previous_wip_mode,
        ApparatusQueuePreviousWipMode::NotRequired
    );
    let source_control = controls
        .get(PECHAT_ID)
        .and_then(|orders| orders.get(order_id))
        .expect("printing control");
    assert_eq!(
        source_control.interaction.opening_wip_mode,
        ApparatusQueuePreviousWipMode::NotRequired
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

    assert_eq!(
        service
            .apply_apparatus_queue_action_with_progress(
                LAMINATION_ID,
                order_id,
                queue_state::ApparatusQueueAction::Start,
                &assigned,
                worker_actor(),
                QueueProgressInput {
                    progress_batch_id: opening.batches[1].batch_id.clone(),
                    qr_payload: opening.batches[0].qr_payload.clone(),
                    ..QueueProgressInput::default()
                },
            )
            .await,
        Err(ProductionMapError::ProgressBatchNotAccepted)
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
            .expect("resume stage starts opening WIP");
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
            .expect("resume stage completes opening WIP roll");
        let processed = service
            .opening_wip_batch(&source.batch_id, "")
            .await
            .expect("processed opening WIP");
        assert_eq!(processed.batch.wip_status, OpeningWipBatchStatus::Processed);

        let states = service
            .apparatus_queue_states()
            .await
            .expect("queue states");
        let state = states
            .get(LAMINATION_ID)
            .and_then(|orders| orders.get(order_id))
            .map(String::as_str);
        if index + 1 < opening.batches.len() {
            assert_eq!(state, Some("pending"));
        } else {
            assert_eq!(state, Some("completed"));
        }
        assert_ne!(
            states
                .get(PECHAT_ID)
                .and_then(|orders| orders.get(order_id))
                .map(String::as_str),
            Some("completed")
        );
    }
}
