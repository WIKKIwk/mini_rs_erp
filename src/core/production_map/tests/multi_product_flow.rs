use super::*;

#[tokio::test]
async fn multi_product_rezka_outputs_reach_stock_as_their_own_skus() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store).await;
    let order_id = "zakaz-mixed-stock";
    let mut map = apparatus_stage_map(order_id, REZKA_ID);
    map.products = crate::core::order_layout::tests::products();
    map.width_mm = Some(575.0);
    map.nodes
        .iter_mut()
        .find(|node| node.kind == ProductionMapNodeKind::Apparatus)
        .unwrap()
        .rezka_kadr_count = Some(6);
    service.upsert_map(map).await.unwrap();
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "mixed-worker".into(),
        display_name: "Worker".into(),
    };
    service
        .apply_apparatus_queue_action_with_progress(
            REZKA_ID,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[REZKA_ID.to_string()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .unwrap();
    let missing_frames = service
        .apply_apparatus_queue_action_with_progress(
            REZKA_ID,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[REZKA_ID.to_string()],
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(100.0),
                gross_qty: Some(11.5),
                diameter: Some(40.0),
                uom: "m".into(),
                ..Default::default()
            },
        )
        .await;
    assert!(matches!(
        missing_frames,
        Err(ProductionMapError::RezkaFrameCountMismatch)
    ));
    let result = service
        .apply_apparatus_queue_action_with_progress(
            REZKA_ID,
            order_id,
            queue_state::ApparatusQueueAction::Pause,
            &[REZKA_ID.to_string()],
            actor,
            QueueProgressInput {
                uom: "m".into(),
                rezka_frames: [115.0, 115.0, 80.0, 80.0, 85.0, 85.0]
                    .into_iter()
                    .map(|width| RezkaFrameProgressInput {
                        produced_qty: Some(100.0),
                        gross_qty: Some(width / 10.0),
                        diameter: Some(40.0),
                        finished_goods_meter: Some(100.0),
                        finished_goods_kg: Some(width / 10.0),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(result.progress_batches.len(), 6);
    let expected = ["SKU-0", "SKU-1", "SKU-2", "SKU-2", "SKU-3", "SKU-3"];
    for (batch, code) in result.progress_batches.iter().zip(expected) {
        assert_eq!(batch.label_item_code, code);
        assert_eq!(
            batch.payload_json["layout_lanes"].as_array().unwrap().len(),
            1
        );
        let receipt = service
            .receive_finished_goods(
                &batch.batch_id,
                &batch.qr_payload,
                "Tayyor mahsulot ombori",
                QueueActionActor {
                    role: "werka".into(),
                    ref_: "mixed-warehouse".into(),
                    display_name: "Warehouse".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(receipt.stock.item_code, code);
        assert_eq!(
            receipt.stock.payload_json["product_id"],
            batch.payload_json["product_id"]
        );
    }
}
