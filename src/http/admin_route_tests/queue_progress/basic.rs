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
            assigned_item_groups: Vec::new(),
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
async fn queue_pause_keeps_state_successful_when_progress_print_fails() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests.clone(),
        fail: true,
    }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-progress-print-fail".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "worker-progress-print-fail",
    )
    .await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-progress-print-fail",
                "Progress print fail",
                "9305",
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
                "order_id":"zakaz-progress-print-fail",
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
                "order_id":"zakaz-progress-print-fail",
                "action":"pause",
                "produced_qty":12,
                "uom":"kg",
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("pause");
    let paused_status = paused.status();
    let paused_body = json_body(paused).await;
    assert_eq!(paused_status, StatusCode::OK, "{paused_body:?}");
    assert_eq!(paused_body["states"]["zakaz-progress-print-fail"], "paused");
    assert_eq!(paused_body["progress_batch"]["status"], "paused");
    assert_eq!(paused_body["print"]["ok"], false);
    assert_eq!(paused_body["print"]["status"], "failed");
    assert_eq!(paused_body["print"]["error"], "printer offline");
    assert_eq!(print_requests.lock().await.len(), 1);
}
