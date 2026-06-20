use super::*;

#[tokio::test]
async fn queue_pause_prints_progress_qr_and_resume_uses_lookup() {
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
            principal_ref: "worker-progress-route".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat".to_string()],
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-progress-route").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-progress-route",
                "Progress order",
                "9301",
                "7 ta rangli pechat",
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
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-progress-route",
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
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-progress-route",
                "action":"pause",
                "produced_qty":15.5,
                "gross_qty":17,
                "uom":"m",
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("pause");
    let paused_status = paused.status();
    let paused_body = json_body(paused).await;
    assert_eq!(paused_status, StatusCode::OK, "{paused_body:?}");
    assert_eq!(paused_body["states"]["zakaz-progress-route"], "paused");
    assert_eq!(paused_body["progress_batch"]["status"], "paused");
    assert_eq!(paused_body["print"]["status"], "printed");
    let qr_payload = paused_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("qr")
        .to_string();
    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 1);
    assert_eq!(printed[0].epc, qr_payload);
    assert!(printed[0].item_name.contains("pauza"));
    assert_eq!(printed[0].executor_name, "Admin");
    assert_eq!(printed[0].gross_qty, 17.0);
    assert_eq!(printed[0].unit, "kg");
    drop(printed);

    let lookup = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/progress-qr/lookup",
            &worker_token,
            &format!(r#"{{"qr_payload":"{qr_payload}"}}"#),
        ))
        .await
        .expect("lookup");
    let lookup_body = json_body(lookup).await;
    assert_eq!(lookup_body["can_resume"], true);
    assert_eq!(lookup_body["batch"]["qr_payload"], qr_payload);

    let resumed = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-progress-route",
                "action":"resume"
            }"#,
        ))
        .await
        .expect("resume");
    let resumed_body = json_body(resumed).await;
    assert_eq!(
        resumed_body["states"]["zakaz-progress-route"],
        "in_progress"
    );
    assert!(resumed_body["progress_batch"].is_null());
}

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

#[tokio::test]
async fn laminatsiya_complete_with_both_leftovers_creates_admin_notice() {
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
            principal_ref: "worker-laminatsiya-notice".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["Laminatsiya 1".to_string()],
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "worker-laminatsiya-notice",
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
                "zakaz-laminatsiya-notice",
                "Laminatsiya notice order",
                "9325",
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
                "order_id":"zakaz-laminatsiya-notice",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"Laminatsiya 1",
                "order_id":"zakaz-laminatsiya-notice",
                "action":"complete",
                "lamination_print_leftover_rolls":1.5,
                "lamination_film_leftover_rolls":2.5,
                "total_waste":3.5,
                "finished_goods_kg":20.75,
                "finished_goods_meter":140.25,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("complete with both leftovers");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");

    let requests = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completion-requests",
            &admin_token,
        ))
        .await
        .expect("completion requests");
    let requests_status = requests.status();
    let requests_body = json_body(requests).await;
    assert_eq!(requests_status, StatusCode::OK, "{requests_body:?}");
    assert_eq!(
        requests_body["completion_requests"][0]["order_id"],
        "zakaz-laminatsiya-notice"
    );
    assert_eq!(
        requests_body["completion_requests"][0]["notice_kind"],
        "laminatsiya_double_leftover"
    );
    assert_eq!(
        requests_body["completion_requests"][0]["decision_required"],
        false
    );
    assert!(
        requests_body["completion_requests"][0]["description"]
            .as_str()
            .unwrap()
            .contains("Bosmadan ortgan rulon: 1.5")
    );
}
