use super::*;

#[tokio::test]
async fn laminatsiya_complete_requires_or_persists_completion_metrics() {
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
            principal_ref: "worker-laminatsiya-complete".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["Laminatsiya 1".to_string()],
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "worker-laminatsiya-complete",
    )
    .await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json_with_dims(
                "zakaz-laminatsiya-complete",
                "Laminatsiya complete order",
                "9323",
                "Laminatsiya 1",
                2.0,
                950.0,
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
                "apparatus":"Laminatsiya 1",
                "order_id":"zakaz-laminatsiya-complete",
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
                "apparatus":"Laminatsiya 1",
                "order_id":"zakaz-laminatsiya-complete",
                "action":"complete"
            }"#,
        ))
        .await
        .expect("complete without metrics");
    assert_eq!(missing_metrics.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_metrics).await["error"],
        "laminatsiya_completion_metrics_required"
    );

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"Laminatsiya 1",
                "order_id":"zakaz-laminatsiya-complete",
                "action":"complete",
                "lamination_film_leftover_rolls":3.5,
                "total_waste":2.25,
                "finished_goods_kg":14.75,
                "finished_goods_meter":110.5,
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
        completed_body["states"]["zakaz-laminatsiya-complete"],
        "completed"
    );
    assert!(completed_body["progress_batch"]["lamination_print_leftover_rolls"].is_null());
    assert_eq!(
        completed_body["progress_batch"]["lamination_film_leftover_rolls"],
        3.5
    );
    assert_eq!(completed_body["progress_batch"]["total_waste"], 2.25);
    assert_eq!(completed_body["progress_batch"]["finished_goods_kg"], 14.75);
    assert_eq!(
        completed_body["progress_batch"]["finished_goods_meter"],
        110.5
    );
    assert_eq!(
        completed_body["progress_event"]["lamination_film_leftover_rolls"],
        3.5
    );
    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 1);
    assert_eq!(printed[0].gross_qty, 14.75);
    assert_eq!(printed[0].qty, Some(110.5));
    assert_eq!(printed[0].unit, "kg");
    assert_eq!(printed[0].progress_unit, "m");
}

#[tokio::test]
async fn laminatsiya_pause_does_not_persist_print_leftover_metric() {
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
            principal_ref: "worker-laminatsiya-pause".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["Laminatsiya 2".to_string()],
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-laminatsiya-pause").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json_with_dims(
                "zakaz-laminatsiya-pause",
                "Laminatsiya pause order",
                "9324",
                "Laminatsiya 2",
                2.0,
                950.0,
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
                "apparatus":"Laminatsiya 2",
                "order_id":"zakaz-laminatsiya-pause",
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
                "apparatus":"Laminatsiya 2",
                "order_id":"zakaz-laminatsiya-pause",
                "action":"pause",
                "lamination_print_leftover_rolls":8,
                "lamination_film_leftover_rolls":4,
                "total_waste":1.5,
                "finished_goods_kg":10,
                "finished_goods_meter":72,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("pause with print leftover ignored");
    let paused_status = paused.status();
    let paused_body = json_body(paused).await;
    assert_eq!(paused_status, StatusCode::OK, "{paused_body:?}");
    assert_eq!(paused_body["progress_batch"]["status"], "paused");
    assert!(paused_body["progress_batch"]["lamination_print_leftover_rolls"].is_null());
    assert_eq!(
        paused_body["progress_batch"]["lamination_film_leftover_rolls"],
        4.0
    );
    assert_eq!(paused_body["progress_batch"]["total_waste"], 1.5);
    assert_eq!(paused_body["progress_batch"]["finished_goods_kg"], 10.0);
    assert_eq!(paused_body["progress_batch"]["finished_goods_meter"], 72.0);
}
