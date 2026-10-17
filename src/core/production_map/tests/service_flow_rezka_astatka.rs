use super::*;

#[tokio::test]
async fn rezka_astatka_uses_the_execution_stage_in_every_work_state() {
    for (status, queue_state) in [
        (OrderRunStatus::Active, "in_progress"),
        (OrderRunStatus::Paused, "paused"),
        (OrderRunStatus::Completed, "completed"),
    ] {
        for (stage, intermediate) in [("cut-before", true), ("cut-final", false)] {
            let store = Arc::new(MemoryProductionMapStore::new());
            let service = default_service_with_store(store.clone()).await;
            let order = "zakaz-rezka-waste-mode";
            let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
                "id": order, "product_code": "WASTE", "title": "Rezka waste mode",
                "nodes": [
                    {"id":"start", "kind":"start", "title":"Start"},
                    {"id":"cut-before", "kind":"apparatus", "title":"Rezka",
                     "apparatus_id":REZKA_ID, "rezka_kadr_count":1},
                    {"id":"lam", "kind":"apparatus", "title":"Lamination",
                     "apparatus_id":LAMINATION_1_ID},
                    {"id":"cut-final", "kind":"apparatus", "title":"Rezka",
                     "apparatus_id":REZKA_ID, "rezka_kadr_count":1},
                    {"id":"end", "kind":"end", "title":"End"}
                ],
                "edges": [
                    {"from":"start", "to":"cut-before"},
                    {"from":"cut-before", "to":"lam"},
                    {"from":"lam", "to":"cut-final"},
                    {"from":"cut-final", "to":"end"}
                ]
            }))
            .unwrap();
            service.upsert_map(map).await.unwrap();
            let actor = QueueActionActor {
                role: "aparatchi".into(),
                ref_: "rezka-worker".into(),
                display_name: "Rezka worker".into(),
            };
            store
                .put_order_run_session(OrderRunSession {
                    session_id: "run".into(),
                    apparatus: REZKA_ID.into(),
                    order_id: order.into(),
                    stage_node_id: stage.into(),
                    status,
                    worker_role: actor.role.clone(),
                    worker_ref: actor.ref_.clone(),
                    worker_display_name: actor.display_name.clone(),
                    started_at_unix: 200,
                    updated_at_unix: 200,
                    payload_json: serde_json::json!({}),
                })
                .await
                .unwrap();
            store
                .put_apparatus_sequence(REZKA_ID, vec![order.into()])
                .await
                .unwrap();
            store
                .put_apparatus_queue_states(
                    REZKA_ID,
                    BTreeMap::from([(order.into(), queue_state.into())]),
                )
                .await
                .unwrap();

            let before = service.live_snapshot().await.unwrap();
            let control = &before.queue_action_controls[REZKA_ID][order];
            assert_eq!(control.stage_node_id, stage);
            assert_eq!(
                control.rezka_total_waste_only, intermediate,
                "{stage} {status:?}"
            );

            // A stale form must not attach its accounting to another occurrence
            // of the same physical machine.
            let other_stage = if intermediate {
                "cut-final"
            } else {
                "cut-before"
            };
            assert_eq!(
                service
                    .record_rezka_astatka(
                        REZKA_ID,
                        order,
                        actor.clone(),
                        Some(1.0),
                        Some(0.0),
                        Some(0.0),
                        Some(0.0),
                        None,
                        None,
                        None,
                        "",
                        other_stage,
                    )
                    .await,
                Err(ProductionMapError::QueueActionNotAllowed)
            );

            assert_eq!(
                service
                    .record_rezka_astatka(
                        REZKA_ID,
                        order,
                        actor.clone(),
                        Some(-1.0),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        "",
                        stage,
                    )
                    .await,
                Err(ProductionMapError::RezkaAstatkaMetricsRequired)
            );
            assert_eq!(
                service
                    .record_rezka_astatka(
                        REZKA_ID,
                        order,
                        actor.clone(),
                        Some(1.0),
                        Some(-1.0),
                        Some(0.0),
                        Some(0.0),
                        None,
                        None,
                        None,
                        "",
                        stage,
                    )
                    .await,
                Err(ProductionMapError::RezkaAstatkaMetricsRequired)
            );

            for waste in [0.0, 2.5] {
                let report = service
                    .record_rezka_astatka(
                        REZKA_ID,
                        order,
                        actor.clone(),
                        Some(waste),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        "",
                        stage,
                    )
                    .await;
                if intermediate {
                    let report = report.unwrap();
                    assert_eq!(report.total_waste, waste);
                    assert_eq!(report.rezka_bosma_waste, 0.0);
                    assert_eq!(report.rezka_lamination_waste, 0.0);
                    assert_eq!(report.rezka_edge_waste, 0.0);
                } else {
                    assert_eq!(report, Err(ProductionMapError::RezkaAstatkaMetricsRequired));
                }
            }
            // Existing clients may still send all categories.
            let report = service
                .record_rezka_astatka(
                    REZKA_ID,
                    order,
                    actor,
                    Some(1.0),
                    Some(2.0),
                    Some(3.0),
                    Some(4.0),
                    None,
                    None,
                    None,
                    "",
                    stage,
                )
                .await
                .unwrap();
            assert_eq!(report.rezka_edge_waste, 4.0);
            assert_eq!(
                store.apparatus_queue_states().await.unwrap()[REZKA_ID][order],
                queue_state
            );
            assert!(
                store
                    .progress_batches_for_order(order)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    }
}
