use super::*;

#[tokio::test]
async fn bosma_complete_requires_or_persists_completion_metrics() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests.clone(),
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-bosma-complete".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli bosma".to_string()],
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-bosma-complete").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-bosma-complete",
                "Bosma complete order",
                "9321",
                "7 ta rangli bosma",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli bosma",
                "order_id":"zakaz-bosma-complete",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let missing_metrics = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli bosma",
                "order_id":"zakaz-bosma-complete",
                "action":"complete"
            }"#,
        ))
        .await
        .expect("complete without metrics");
    assert_eq!(missing_metrics.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_metrics).await["error"],
        "bosma_completion_metrics_required"
    );

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli bosma",
                "order_id":"zakaz-bosma-complete",
                "action":"complete",
                "return_ink_kg":1.25,
                "total_waste":2.5,
                "finished_goods_kg":18.75,
                "finished_goods_meter":125.5,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("complete with metrics");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");
    assert_eq!(
        completed_body["states"]["zakaz-bosma-complete"],
        "completed"
    );
    assert_eq!(completed_body["progress_batch"]["produced_qty"], 125.5);
    assert_eq!(completed_body["progress_batch"]["uom"], "m");
    assert_eq!(completed_body["progress_batch"]["return_ink_kg"], 1.25);
    assert_eq!(completed_body["progress_batch"]["total_waste"], 2.5);
    assert_eq!(completed_body["progress_batch"]["finished_goods_kg"], 18.75);
    assert_eq!(
        completed_body["progress_batch"]["finished_goods_meter"],
        125.5
    );
    assert_eq!(
        completed_body["progress_event"]["finished_goods_meter"],
        125.5
    );
    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 1);
    assert_eq!(printed[0].gross_qty, 18.75);
    assert_eq!(printed[0].qty, Some(125.5));
    assert_eq!(printed[0].unit, "kg");
    assert_eq!(printed[0].progress_unit, "m");
}

#[tokio::test]
async fn bosma_pause_does_not_persist_return_ink_metric() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests,
        fail: false,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-bosma-pause".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["8 ta rangli bosma".to_string()],
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-bosma-pause").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-bosma-pause",
                "Bosma pause order",
                "9322",
                "8 ta rangli bosma",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"8 ta rangli bosma",
                "order_id":"zakaz-bosma-pause",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let paused = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"8 ta rangli bosma",
                "order_id":"zakaz-bosma-pause",
                "action":"pause",
                "finished_goods_kg":12,
                "finished_goods_meter":80,
                "return_ink_kg":9,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("pause with return ink ignored");
    let paused_status = paused.status();
    let paused_body = json_body(paused).await;
    assert_eq!(paused_status, StatusCode::OK, "{paused_body:?}");
    assert_eq!(paused_body["progress_batch"]["status"], "paused");
    assert!(paused_body["progress_batch"]["return_ink_kg"].is_null());
    assert_eq!(paused_body["progress_batch"]["finished_goods_kg"], 12.0);
    assert_eq!(paused_body["progress_batch"]["finished_goods_meter"], 80.0);
}
