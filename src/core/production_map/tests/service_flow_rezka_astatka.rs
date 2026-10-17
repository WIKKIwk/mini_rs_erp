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
                        "run",
                        "test-request-1",
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
                        "run",
                        "test-request-2",
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
                        "run",
                        "test-request-3",
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
                        "run",
                        &format!("waste-report-{waste}"),
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
                    "run",
                    "test-request-5",
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

async fn receipt_fixture() -> (
    ProductionMapService,
    Arc<MemoryProductionMapStore>,
    QueueActionActor,
) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    service.upsert_map(serde_json::from_value(serde_json::json!({
        "id":"zakaz-rezka-receipt", "product_code":"RECEIPT", "title":"Receipts",
        "nodes":[
            {"id":"start","kind":"start","title":"Start"},
            {"id":"cut","kind":"apparatus","title":"Cut","apparatus_id":REZKA_ID},
            {"id":"lam","kind":"apparatus","title":"Lamination","apparatus_id":LAMINATION_1_ID},
            {"id":"end","kind":"end","title":"End"}
        ],
        "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"lam"},{"from":"lam","to":"end"}]
    })).unwrap()).await.unwrap();
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "cut-worker".into(),
        display_name: "Worker".into(),
    };
    store
        .put_order_run_session(OrderRunSession {
            session_id: "original-run".into(),
            order_id: "zakaz-rezka-receipt".into(),
            apparatus: REZKA_ID.into(),
            stage_node_id: "cut".into(),
            status: OrderRunStatus::Completed,
            started_at_unix: 100,
            updated_at_unix: 101,
            worker_role: actor.role.clone(),
            worker_ref: actor.ref_.clone(),
            worker_display_name: actor.display_name.clone(),
            payload_json: serde_json::json!({"stage_work_protocol":1}),
        })
        .await
        .unwrap();
    (service, store, actor)
}

async fn receipt_submit(
    service: &ProductionMapService,
    actor: &QueueActionActor,
    session: &str,
    request: &str,
    waste: f64,
) -> Result<RezkaAstatkaReport, ProductionMapError> {
    service
        .record_rezka_astatka(
            REZKA_ID,
            "zakaz-rezka-receipt",
            actor.clone(),
            Some(waste),
            None,
            None,
            None,
            None,
            None,
            None,
            "",
            "cut",
            session,
            request,
        )
        .await
}

#[tokio::test]
async fn rezka_astatka_retry_is_one_report_and_distinct_reports_still_add() {
    let (service, store, actor) = receipt_fixture().await;
    let (first, retry) = tokio::join!(
        receipt_submit(
            &service,
            &actor,
            "original-run",
            "lost-response-request",
            2.5
        ),
        receipt_submit(
            &service,
            &actor,
            "original-run",
            "lost-response-request",
            2.5
        ),
    );
    let first = first.unwrap();
    assert_eq!(first, retry.unwrap());
    assert_eq!(
        receipt_submit(
            &service,
            &actor,
            "original-run",
            "lost-response-request",
            9.0
        )
        .await,
        Err(ProductionMapError::ProgressInputInvalid)
    );
    let reports = store
        .rezka_astatka_reports_for_order("zakaz-rezka-receipt")
        .await
        .unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].total_waste, 2.5);
    receipt_submit(
        &service,
        &actor,
        "original-run",
        "additional-report-request",
        0.5,
    )
    .await
    .unwrap();
    let reports = store
        .rezka_astatka_reports_for_order("zakaz-rezka-receipt")
        .await
        .unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports.iter().map(|r| r.total_waste).sum::<f64>(), 3.0);
}

#[tokio::test]
async fn rezka_astatka_stale_form_rejected_but_committed_receipt_survives_reactivation() {
    let (service, store, actor) = receipt_fixture().await;
    let snapshot = service
        .rezka_astatka_snapshot(REZKA_ID, "zakaz-rezka-receipt", &actor, "")
        .await
        .unwrap();
    assert_eq!(snapshot["session_id"], "original-run");
    let saved = receipt_submit(
        &service,
        &actor,
        "original-run",
        "saved-report-request",
        2.5,
    )
    .await
    .unwrap();
    let mut replacement = store
        .order_run_sessions_for_order("zakaz-rezka-receipt")
        .await
        .unwrap()[0]
        .clone();
    replacement.session_id = "reactivated-run".into();
    replacement.started_at_unix = 102;
    replacement.updated_at_unix = 103;
    replacement.payload_json = serde_json::json!({"stage_work_protocol":1});
    store.put_order_run_session(replacement).await.unwrap();
    assert_eq!(
        receipt_submit(&service, &actor, "original-run", "stale-form-request", 5.0).await,
        Err(ProductionMapError::QueueActionNotAllowed)
    );
    assert_eq!(
        receipt_submit(
            &service,
            &actor,
            "original-run",
            "saved-report-request",
            2.5
        )
        .await
        .unwrap(),
        saved
    );
    let replay = service
        .rezka_astatka_snapshot(
            REZKA_ID,
            "zakaz-rezka-receipt",
            &actor,
            "saved-report-request",
        )
        .await
        .unwrap();
    assert_eq!(
        replay["request_result"]["report"]["report_id"],
        saved.report_id
    );
    let current = store
        .order_run_sessions_for_order("zakaz-rezka-receipt")
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.session_id == "reactivated-run")
        .unwrap();
    assert!(current.payload_json.get("stage_work_report").is_none());
    assert_eq!(
        store
            .rezka_astatka_reports_for_order("zakaz-rezka-receipt")
            .await
            .unwrap()
            .len(),
        1
    );
    receipt_submit(
        &service,
        &actor,
        "reactivated-run",
        "new-session-request",
        0.0,
    )
    .await
    .unwrap();
    assert_eq!(
        receipt_submit(&service, &actor, "", "missing-session-request", 1.0).await,
        Err(ProductionMapError::ProgressInputInvalid)
    );
    assert_eq!(
        receipt_submit(&service, &actor, "reactivated-run", "", 1.0).await,
        Err(ProductionMapError::ProgressInputInvalid)
    );
}
