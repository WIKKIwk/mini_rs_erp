use super::*;

#[tokio::test]
async fn wip_batches_endpoint_lists_waiting_and_in_use_batches() {
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
            principal_ref: "worker-wip-route".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec![
                "7 ta rangli pechat".to_string(),
                "Laminatsiya mashinasi".to_string(),
            ],
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-wip-route").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &two_apparatus_order_map_json(
                "zakaz-wip-route",
                "WIP route order",
                "9401",
                "7 ta rangli pechat",
                "Laminatsiya mashinasi",
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
                "order_id":"zakaz-wip-route",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start first");
    assert_eq!(started.status(), StatusCode::OK);

    let paused = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-wip-route",
                "action":"pause",
                "produced_qty":100,
                "uom":"kg"
            }"#,
        ))
        .await
        .expect("pause first");
    assert_eq!(paused.status(), StatusCode::OK);
    let paused_body = json_body(paused).await;
    let qr_payload = paused_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("qr payload")
        .to_string();

    let waiting = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=7%20ta%20rangli%20pechat&status=waiting",
            &admin_token,
        ))
        .await
        .expect("waiting wip");
    let waiting_body = json_body(waiting).await;
    assert_eq!(waiting_body["batches"][0]["qr_payload"], qr_payload);
    assert_eq!(waiting_body["batches"][0]["wip_status"], "waiting");
    assert_eq!(
        waiting_body["batches"][0]["current_apparatus"],
        "7 ta rangli pechat"
    );

    let second_started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"Laminatsiya mashinasi",
                    "order_id":"zakaz-wip-route",
                    "action":"start",
                    "qr_payload":"{qr_payload}"
                }}"#
            ),
        ))
        .await
        .expect("start second");
    assert_eq!(second_started.status(), StatusCode::OK);

    let in_use = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=Laminatsiya%20mashinasi&status=in_use",
            &admin_token,
        ))
        .await
        .expect("in-use wip");
    let in_use_body = json_body(in_use).await;
    assert_eq!(in_use_body["batches"][0]["qr_payload"], qr_payload);
    assert_eq!(in_use_body["batches"][0]["wip_status"], "in_use");
    assert_eq!(
        in_use_body["batches"][0]["current_apparatus"],
        "Laminatsiya mashinasi"
    );
    assert_eq!(
        in_use_body["batches"][0]["used_by_apparatus"],
        "Laminatsiya mashinasi"
    );
}

#[tokio::test]
async fn wip_batches_endpoint_forbids_worker_unassigned_or_unscoped_listing() {
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
            principal_ref: "worker-wip-scope".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat".to_string()],
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-wip-scope").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &two_apparatus_order_map_json(
                "zakaz-wip-scope",
                "WIP scope order",
                "9402",
                "7 ta rangli pechat",
                "Laminatsiya mashinasi",
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
                "order_id":"zakaz-wip-scope",
                "action":"start"
            }"#,
        ))
        .await
        .expect("start first");
    assert_eq!(started.status(), StatusCode::OK);

    let paused = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-wip-scope",
                "action":"pause",
                "produced_qty":100,
                "uom":"kg"
            }"#,
        ))
        .await
        .expect("pause first");
    assert_eq!(paused.status(), StatusCode::OK);

    let unscoped = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?status=waiting",
            &worker_token,
        ))
        .await
        .expect("unscoped wip");
    assert_eq!(unscoped.status(), StatusCode::FORBIDDEN);

    let unassigned = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=Laminatsiya%20mashinasi&status=waiting",
            &worker_token,
        ))
        .await
        .expect("unassigned wip");
    assert_eq!(unassigned.status(), StatusCode::FORBIDDEN);
}
