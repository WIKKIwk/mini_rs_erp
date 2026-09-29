use super::*;

#[tokio::test]
async fn progress_qr_usage_reports_consumed_and_busy_rolls_without_changing_ownership() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let order = "zakaz-qr-usage";
    let mut map = two_stage_map(order, LAMINATION_1_ID, REZKA_ID);
    map.nodes.iter_mut().find(|node| node.id == "second").unwrap().title = "Rezka 5".into();
    service.upsert_map(map).await.unwrap();

    for (wip_status, expected) in [
        (OrderProgressBatchWipStatus::Processed,
            ProductionMapError::ProgressBatchAlreadyUsed { apparatus_name: "Rezka 5".into() }),
        (OrderProgressBatchWipStatus::InUse,
            ProductionMapError::ProgressBatchInUse { apparatus_name: "Rezka 5".into() }),
    ] {
        let mut batch = test_progress_batch("batch-qr-usage", order, LAMINATION_1_ID,
            "QR-USAGE", wip_status, "");
        batch.next_apparatus = REZKA_ID.into();
        batch.current_apparatus = REZKA_ID.into();
        batch.used_by_apparatus = REZKA_ID.into();
        batch.used_by_session_id = "consumer-session".into();
        if wip_status == OrderProgressBatchWipStatus::Processed {
            batch.processed_by_apparatus = REZKA_ID.into();
            batch.processed_by_session_id = "consumer-session".into();
        }
        store.put_order_progress_batch(batch.clone()).await.unwrap();
        assert_eq!(service.start_input_for_qr(REZKA_ID, order, "", "QR-USAGE").await,
            Err(expected));
        let persisted = store.progress_batch(&batch.batch_id).await.unwrap().unwrap();
        assert_eq!(persisted.wip_status, wip_status);
        assert_eq!(persisted.used_by_session_id, "consumer-session");
        // A genuinely wrong production stage must not become a usage error.
        assert_eq!(service.start_input_for_qr(LAMINATION_1_ID, order, "", "QR-USAGE").await,
            Err(ProductionMapError::ProgressBatchNotAccepted));
    }

    let mut waiting = test_progress_batch("batch-waiting", order, LAMINATION_1_ID,
        "QR-WAITING", OrderProgressBatchWipStatus::Waiting, "");
    waiting.next_apparatus = REZKA_ID.into();
    store.put_order_progress_batch(waiting).await.unwrap();
    assert_eq!(service.start_input_for_qr(REZKA_ID, order, "", "QR-WAITING").await.unwrap().wip_status,
        OrderProgressBatchWipStatus::Waiting);
}
