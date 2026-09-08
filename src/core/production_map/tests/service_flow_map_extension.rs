use super::*;

async fn detached_final_stage_roll(
    store: Arc<MemoryProductionMapStore>,
    order_id: &str,
) -> (ProductionMapService, OrderProgressBatch, QueueActionActor) {
    let service = default_service_with_store(store).await;
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "worker-map-extension".into(),
        display_name: "Map extension worker".into(),
    };
    service.upsert_map(apparatus_stage_map(order_id, FLOW_PECHAT_ID))
        .await.expect("print-only order");
    start_first_stage(&service, order_id, FLOW_PECHAT_ID, actor.clone())
        .await.expect("start printing");
    let batch = service.apply_apparatus_queue_action_with_progress(
        FLOW_PECHAT_ID, order_id, queue_state::ApparatusQueueAction::DetachRoll,
        &[FLOW_PECHAT_ID.to_string()], actor.clone(),
        QueueProgressInput {
            produced_qty: Some(454.0), uom: "m".into(),
            ..QueueProgressInput::default()
        },
    ).await.expect("detach printed roll").progress_batch.expect("WIP");
    assert!(batch.next_apparatus.is_empty());
    assert_eq!(batch.payload_json["next_stage_node_id"], "");
    (service, batch, actor)
}

async fn waiting_for(
    service: &ProductionMapService, order_id: &str, next: &str,
) -> Vec<OrderProgressBatch> {
    service.wip_progress_batches(WipProgressBatchQuery::new(
        FLOW_PECHAT_ID, next, "", Some(OrderProgressBatchWipStatus::Waiting),
        false, order_id, 250,
    )).await.expect("waiting inputs")
}

#[tokio::test]
async fn map_extension_detached_roll_is_visible_and_claimed_once_by_added_lamination() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let order_id = "zakaz-map-extension";
    let (service, batch, actor) = detached_final_stage_roll(store.clone(), order_id).await;
    service.upsert_map(unassigned_alternative_next_stage_map(
        order_id, FLOW_PECHAT_ID, LAMINATION_1_ID, LAMINATION_2_ID,
    )).await.expect("append laminators after printing");
    for next in [LAMINATION_1_ID, LAMINATION_2_ID] {
        let inputs = waiting_for(&service, order_id, next).await;
        assert_eq!(inputs.len(), 1, "existing WIP must be visible to {next}");
        assert_eq!(inputs[0].batch_id, batch.batch_id);
        assert_eq!(inputs[0].qr_payload, batch.qr_payload);
        assert_eq!(inputs[0].produced_qty, 454.0);
    }
    assert!(store.progress_batch(&batch.batch_id).await.unwrap().unwrap()
        .next_apparatus.is_empty(), "GET must not rewrite persisted history");
    let progress = QueueProgressInput {
        progress_batch_id: batch.batch_id.clone(), qr_payload: batch.qr_payload.clone(),
        ..QueueProgressInput::default()
    };
    let assigned_1 = [LAMINATION_1_ID.to_string()];
    let assigned_2 = [LAMINATION_2_ID.to_string()];
    let (first, second) = tokio::join!(
        service.apply_apparatus_queue_action_with_progress(
            LAMINATION_1_ID, order_id, queue_state::ApparatusQueueAction::Start,
            &assigned_1, actor.clone(), progress.clone(),
        ),
        service.apply_apparatus_queue_action_with_progress(
            LAMINATION_2_ID, order_id, queue_state::ApparatusQueueAction::Start,
            &assigned_2, actor, progress,
        ),
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1,
        "exactly one laminator can claim the existing roll: {first:?}, {second:?}");
    let claimed = store.progress_batch(&batch.batch_id).await.unwrap().unwrap();
    assert_eq!(claimed.wip_status, OrderProgressBatchWipStatus::InUse);
    assert!(!claimed.used_by_session_id.is_empty());
    assert_eq!(claimed.produced_qty, batch.produced_qty);
    assert_eq!(claimed.qr_payload, batch.qr_payload);
    for next in [LAMINATION_1_ID, LAMINATION_2_ID] {
        assert!(waiting_for(&service, order_id, next).await.is_empty());
    }
}

#[tokio::test]
async fn map_extension_does_not_offer_consumed_or_explicitly_routed_rolls() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let order_id = "zakaz-map-extension-protected";
    let (service, original, _) = detached_final_stage_roll(store.clone(), order_id).await;
    service.upsert_map(two_stage_map(order_id, FLOW_PECHAT_ID, LAMINATION_1_ID))
        .await.expect("append laminator");
    for status in [OrderProgressBatchWipStatus::InUse, OrderProgressBatchWipStatus::Processed] {
        let mut batch = original.clone();
        batch.wip_status = status;
        batch.used_by_session_id = "other-session".into();
        batch.used_by_apparatus = LAMINATION_2_ID.into();
        store.put_order_progress_batch(batch).await.unwrap();
        assert!(waiting_for(&service, order_id, LAMINATION_1_ID).await.is_empty());
    }
    let mut batch = original.clone();
    batch.next_apparatus = LAMINATION_2_ID.into();
    store.put_order_progress_batch(batch).await.unwrap();
    assert!(waiting_for(&service, order_id, LAMINATION_1_ID).await.is_empty());
    assert_eq!(store.progress_batch(&original.batch_id).await.unwrap().unwrap()
        .next_apparatus, LAMINATION_2_ID);
}

#[tokio::test]
async fn map_extension_missing_source_occurrence_is_not_guessed() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let order_id = "zakaz-map-extension-identity";
    let (service, mut batch, _) = detached_final_stage_roll(store.clone(), order_id).await;
    service.upsert_map(two_stage_map(order_id, FLOW_PECHAT_ID, LAMINATION_1_ID))
        .await.expect("append laminator");
    batch.payload_json["stage_node_id"] = serde_json::json!("removed-print-occurrence");
    store.put_order_progress_batch(batch).await.unwrap();
    assert!(waiting_for(&service, order_id, LAMINATION_1_ID).await.is_empty());
}

#[tokio::test]
async fn map_extension_completion_waits_for_every_preexisting_roll() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let order_id = "zakaz-map-extension-completion";
    let (service, first, actor) = detached_final_stage_roll(store.clone(), order_id).await;
    let mut second = first.clone();
    second.batch_id = "map-extension-second-roll".into();
    second.qr_payload = "map-extension-second-qr".into();
    store.put_order_progress_batch(second.clone()).await.unwrap();
    store.put_apparatus_queue_states(FLOW_PECHAT_ID,
        BTreeMap::from([(order_id.into(), "completed".into())]))
        .await.expect("producer has finished all rolls");
    service.upsert_map(two_stage_map(order_id, FLOW_PECHAT_ID, LAMINATION_1_ID))
        .await.expect("append laminator after output");
    for (input, expected_state) in [(&first, "pending"), (&second, "completed")] {
        service.apply_apparatus_queue_action_with_progress(
            LAMINATION_1_ID, order_id, queue_state::ApparatusQueueAction::Start,
            &[LAMINATION_1_ID.into()], actor.clone(),
            QueueProgressInput { qr_payload: input.qr_payload.clone(), ..QueueProgressInput::default() },
        ).await.expect("accept old roll");
        let result = service.apply_apparatus_queue_action_with_progress(
            LAMINATION_1_ID, order_id, queue_state::ApparatusQueueAction::Complete,
            &[LAMINATION_1_ID.into()], actor.clone(),
            QueueProgressInput {
                produced_qty: Some(11.0), uom: "kg".into(),
                finished_goods_kg: Some(11.0), finished_goods_meter: Some(454.0),
                lamination_film_leftover_rolls: Some(1.0), total_waste: Some(0.5),
                ..QueueProgressInput::default()
            },
        ).await.expect("complete one input roll");
        assert_eq!(result.states.get(order_id).map(String::as_str), Some(expected_state));
        assert_eq!(store.progress_batch(&input.batch_id).await.unwrap().unwrap().wip_status,
            OrderProgressBatchWipStatus::Processed);
        if expected_state == "pending" {
            assert_eq!(waiting_for(&service, order_id, LAMINATION_1_ID).await.len(), 1);
        }
    }
    assert!(waiting_for(&service, order_id, LAMINATION_1_ID).await.is_empty());
}
