use super::*;

#[tokio::test]
async fn rezka_complete_requires_or_persists_progress_metrics() {
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
            principal_ref: "worker-rezka-complete".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["Rezka".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-rezka-complete").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-rezka-complete",
                "Rezka complete order",
                "9325",
                "Rezka",
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
                "apparatus":"Rezka",
                "order_id":"zakaz-rezka-complete",
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
                "apparatus":"Rezka",
                "order_id":"zakaz-rezka-complete",
                "action":"complete",
                "produced_qty":32,
                "gross_qty":32,
                "uom":"kg"
            }"#,
        ))
        .await
        .expect("complete without rezka metrics");
    assert_eq!(missing_metrics.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_metrics).await["error"],
        "rezka_progress_metrics_required"
    );

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"Rezka",
                "order_id":"zakaz-rezka-complete",
                "action":"complete",
                "produced_qty":32,
                "gross_qty":32,
                "uom":"kg",
                "rezka_bosma_waste":1.25,
                "rezka_lamination_waste":2.5,
                "rezka_edge_waste":0.75,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("complete with rezka metrics");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");
    assert_eq!(
        completed_body["states"]["zakaz-rezka-complete"],
        "completed"
    );
    assert_eq!(completed_body["progress_batch"]["rezka_bosma_waste"], 1.25);
    assert_eq!(
        completed_body["progress_batch"]["rezka_lamination_waste"],
        2.5
    );
    assert_eq!(completed_body["progress_batch"]["rezka_edge_waste"], 0.75);
    assert_eq!(completed_body["progress_event"]["rezka_edge_waste"], 0.75);
    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 1);
    assert_eq!(printed[0].gross_qty, 32.0);
}

#[tokio::test]
async fn rezka_pause_requires_and_persists_progress_metrics() {
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
            principal_ref: "worker-rezka-pause".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["Rezka".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-rezka-pause").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json("zakaz-rezka-pause", "Rezka pause order", "9326", "Rezka"),
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
                "apparatus":"Rezka",
                "order_id":"zakaz-rezka-pause",
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
                "apparatus":"Rezka",
                "order_id":"zakaz-rezka-pause",
                "action":"pause",
                "produced_qty":18,
                "gross_qty":18,
                "uom":"kg"
            }"#,
        ))
        .await
        .expect("pause without rezka metrics");
    assert_eq!(missing_metrics.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_metrics).await["error"],
        "rezka_progress_metrics_required"
    );

    let paused = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"Rezka",
                "order_id":"zakaz-rezka-pause",
                "action":"pause",
                "produced_qty":18,
                "gross_qty":18,
                "uom":"kg",
                "rezka_bosma_waste":1,
                "rezka_lamination_waste":2,
                "rezka_edge_waste":3,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("pause with rezka metrics");
    let paused_status = paused.status();
    let paused_body = json_body(paused).await;
    assert_eq!(paused_status, StatusCode::OK, "{paused_body:?}");
    assert_eq!(paused_body["progress_batch"]["status"], "paused");
    assert_eq!(paused_body["progress_batch"]["rezka_bosma_waste"], 1.0);
    assert_eq!(paused_body["progress_batch"]["rezka_lamination_waste"], 2.0);
    assert_eq!(paused_body["progress_batch"]["rezka_edge_waste"], 3.0);
}
