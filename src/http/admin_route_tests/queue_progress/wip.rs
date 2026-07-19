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
            assigned_item_groups: Vec::new(),
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

    provision_test_qolip(&router, &admin_token, "zakaz-wip-route").await;

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-wip-route",
                "action":"start"
            }"#,
                "zakaz-wip-route",
            ),
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
    assert_eq!(
        waiting_body["batches"][0]["current_location"],
        "7 ta rangli pechat chiqim"
    );

    let waiting_by_location = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?current_location=7%20ta%20rangli%20pechat%20chiqim&status=waiting",
            &admin_token,
        ))
        .await
        .expect("waiting wip by location");
    let waiting_by_location_body = json_body(waiting_by_location).await;
    assert_eq!(
        waiting_by_location_body["batches"][0]["qr_payload"],
        qr_payload
    );
    assert_eq!(
        waiting_by_location_body["batches"][0]["current_location"],
        "7 ta rangli pechat chiqim"
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
        .clone()
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

    let all_for_next = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=7%20ta%20rangli%20pechat&next_apparatus=Laminatsiya%20mashinasi&status=all",
            &worker_token,
        ))
        .await
        .expect("all wip for next apparatus");
    let all_for_next_body = json_body(all_for_next).await;
    assert_eq!(all_for_next_body["batches"][0]["qr_payload"], qr_payload);
    assert_eq!(all_for_next_body["batches"][0]["wip_status"], "in_use");
}

#[tokio::test]
async fn wip_batches_endpoint_lists_batches_for_assigned_next_apparatus() {
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
            principal_ref: "worker-wip-next".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["Laminatsiya 1".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-wip-first".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("first assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let lamin_token = session_for(&state, PrincipalRole::Aparatchi, "worker-wip-next").await;
    let first_token = session_for(&state, PrincipalRole::Aparatchi, "worker-wip-first").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &two_apparatus_order_map_json(
                "zakaz-wip-next",
                "WIP next order",
                "9406",
                "7 ta rangli pechat",
                "Laminatsiya 1",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    provision_test_qolip(&router, &admin_token, "zakaz-wip-next").await;

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &first_token,
            &with_test_qolip(
                r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-wip-next",
                "action":"start"
            }"#,
                "zakaz-wip-next",
            ),
        ))
        .await
        .expect("start first");
    assert_eq!(started.status(), StatusCode::OK);

    let paused = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &first_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-wip-next",
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

    let listed = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=7%20ta%20rangli%20pechat&next_apparatus=Laminatsiya%201&order_id=zakaz-wip-next&status=waiting",
            &lamin_token,
        ))
        .await
        .expect("list next apparatus wip");
    let listed_status = listed.status();
    let listed_body = json_body(listed).await;
    assert_eq!(listed_status, StatusCode::OK, "{listed_body:?}");
    assert_eq!(listed_body["batches"][0]["qr_payload"], qr_payload);
    assert_eq!(listed_body["batches"][0]["next_apparatus"], "Laminatsiya 1");
}

#[tokio::test]
async fn complete_after_wip_start_does_not_reuse_input_qr_as_output_qr() {
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
            principal_ref: "worker-wip-complete-qr".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec![
                "7 ta rangli pechat".to_string(),
                "Laminatsiya 1".to_string(),
            ],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-wip-complete-qr").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &two_apparatus_order_map_json(
                "zakaz-wip-complete-qr",
                "WIP complete QR",
                "9405",
                "7 ta rangli pechat",
                "Laminatsiya 1",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    provision_test_qolip(&router, &admin_token, "zakaz-wip-complete-qr").await;

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-wip-complete-qr",
                "action":"start"
            }"#,
                "zakaz-wip-complete-qr",
            ),
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
                "order_id":"zakaz-wip-complete-qr",
                "action":"pause",
                "produced_qty":100,
                "uom":"kg"
            }"#,
        ))
        .await
        .expect("pause first");
    assert_eq!(paused.status(), StatusCode::OK);
    let paused_body = json_body(paused).await;
    let input_qr = paused_body["progress_batch"]["qr_payload"]
        .as_str()
        .expect("qr payload")
        .to_string();

    let second_started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"Laminatsiya 1",
                    "order_id":"zakaz-wip-complete-qr",
                    "action":"start",
                    "qr_payload":"{input_qr}"
                }}"#
            ),
        ))
        .await
        .expect("start second");
    assert_eq!(second_started.status(), StatusCode::OK);

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"Laminatsiya 1",
                    "order_id":"zakaz-wip-complete-qr",
                    "action":"complete",
                    "qr_payload":"{input_qr}",
                    "lamination_film_leftover_rolls":1,
                    "total_waste":1,
                    "finished_goods_kg":9,
                    "finished_goods_meter":90,
                    "printer":"zebra",
                    "print_mode":"rfid"
                }}"#
            ),
        ))
        .await
        .expect("complete second");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");
    assert_eq!(
        completed_body["states"]["zakaz-wip-complete-qr"],
        "completed"
    );
    assert_ne!(completed_body["progress_batch"]["qr_payload"], input_qr);
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
            assigned_item_groups: Vec::new(),
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

    provision_test_qolip(&router, &admin_token, "zakaz-wip-scope").await;

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-wip-scope",
                "action":"start"
            }"#,
                "zakaz-wip-scope",
            ),
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
