use super::*;

#[tokio::test]
async fn scoped_progress_qr_lookup_accepts_assigned_alternative_without_claiming() {
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: Arc::new(Mutex::new(Vec::new())),
        fail: false,
    }));
    let lam1 = "apparatus:default:asset-007";
    let lam2 = "apparatus:default:asset-008";
    let print = "apparatus:default:bosma_7";
    let order = "zakaz-scoped-qr";
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "lam1-qr-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![lam1.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "print-qr-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![print.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let admin = session(&state, PrincipalRole::Admin).await;
    let worker = session_for(&state, PrincipalRole::Aparatchi, "lam1-qr-worker").await;
    let printer = session_for(&state, PrincipalRole::Aparatchi, "print-qr-worker").await;
    let router = build_router(state);
    let mut map: serde_json::Value = serde_json::from_str(&two_apparatus_order_map_json(
        order,
        "Alternative QR",
        "9409",
        print,
        lam2,
    ))
    .unwrap();
    map["nodes"][2]["alternative_group_id"] = serde_json::json!("lam-stage");
    map["nodes"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id":"peer", "kind":"apparatus", "title":"Lam1", "apparatus_id":lam1,
            "alternative_group_id":"lam-stage"
        }));
    map["edges"].as_array_mut().unwrap().extend([
        serde_json::json!({"from":"first", "to":"peer"}),
        serde_json::json!({"from":"peer", "to":"end"}),
    ]);
    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin,
            &map.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK, "{}", json_body(saved).await);
    provision_test_qolip(&router, &admin, order).await;
    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &printer,
            &with_test_qolip(
                &serde_json::json!({"apparatus":print,"order_id":order,"action":"start"})
                    .to_string(),
                order,
            ),
        ))
        .await
        .unwrap();
    assert_eq!(
        started.status(),
        StatusCode::OK,
        "{}",
        json_body(started).await
    );
    let detached = router.clone().oneshot(request_with_body("POST", "/v1/mobile/admin/production-maps/queue-action", &printer,
        &serde_json::json!({"apparatus":print,"order_id":order,"action":"pause","produced_qty":9900,"uom":"m"}).to_string())).await.unwrap();
    assert_eq!(detached.status(), StatusCode::OK);
    let batch = json_body(detached).await["progress_batch"].clone();
    assert_eq!(batch["next_apparatus"], lam2);
    for (machine, target_order, expected) in [
        (lam1, order, StatusCode::OK),
        (lam2, order, StatusCode::FORBIDDEN),
        (lam1, "", StatusCode::BAD_REQUEST),
        (lam1, "other-order", StatusCode::NOT_FOUND),
        (lam1, order, StatusCode::OK),
    ] {
        let response = router.clone().oneshot(request_with_body("POST", "/v1/mobile/admin/production-maps/progress-qr/lookup", &worker,
            &serde_json::json!({"qr_payload":batch["qr_payload"], "apparatus":machine, "order_id":target_order}).to_string())).await.unwrap();
        let status = response.status();
        let body = json_body(response).await;
        assert_eq!(status, expected, "{machine} {target_order}: {body}");
        if status == StatusCode::OK {
            assert_eq!(body["validated_apparatus"], lam1);
            assert_eq!(body["validated_order_id"], order);
            assert_eq!(body["batch"]["wip_status"], "waiting");
            assert_eq!(body["batch"]["next_apparatus"], lam2);
        }
    }
    let start = router.clone().oneshot(request_with_body("POST", "/v1/mobile/admin/production-maps/queue-action", &worker,
        &serde_json::json!({"apparatus":lam1,"order_id":order,"action":"start","qr_payload":batch["qr_payload"]}).to_string())).await.unwrap();
    assert_eq!(start.status(), StatusCode::OK, "{}", json_body(start).await);
    let claimed = router.oneshot(request_with_body("POST", "/v1/mobile/admin/production-maps/progress-qr/lookup", &admin,
        &serde_json::json!({"qr_payload":batch["qr_payload"],"apparatus":lam2,"order_id":order}).to_string())).await.unwrap();
    assert_eq!(claimed.status(), StatusCode::BAD_REQUEST);
}

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
                "apparatus:default:bosma_7".to_string(),
                "apparatus:default:asset-007".to_string(),
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
                "apparatus:default:bosma_7",
                "apparatus:default:asset-007",
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
                "apparatus":"apparatus:default:bosma_7",
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
                "apparatus":"apparatus:default:bosma_7",
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
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=apparatus%3Adefault%3Abosma_7&status=waiting",
            &admin_token,
        ))
        .await
        .expect("waiting wip");
    let waiting_body = json_body(waiting).await;
    assert_eq!(waiting_body["batches"][0]["qr_payload"], qr_payload);
    assert_eq!(waiting_body["batches"][0]["wip_status"], "waiting");
    assert_eq!(
        waiting_body["batches"][0]["current_apparatus"],
        "apparatus:default:bosma_7"
    );
    assert_eq!(
        waiting_body["batches"][0]["current_location"],
        "apparatus:default:bosma_7 chiqim"
    );

    let waiting_by_location = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?current_location=apparatus%3Adefault%3Abosma_7%20chiqim&status=waiting",
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
        "apparatus:default:bosma_7 chiqim"
    );

    let second_started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"apparatus:default:asset-007",
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
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=apparatus%3Adefault%3Aasset-007&status=in_use",
            &admin_token,
        ))
        .await
        .expect("in-use wip");
    let in_use_body = json_body(in_use).await;
    assert_eq!(in_use_body["batches"][0]["qr_payload"], qr_payload);
    assert_eq!(in_use_body["batches"][0]["wip_status"], "in_use");
    assert_eq!(
        in_use_body["batches"][0]["current_apparatus"],
        "apparatus:default:asset-007"
    );
    assert_eq!(
        in_use_body["batches"][0]["used_by_apparatus"],
        "apparatus:default:asset-007"
    );

    let all_for_next = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=apparatus%3Adefault%3Abosma_7&next_apparatus=apparatus%3Adefault%3Aasset-007&status=all",
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
            assigned_apparatus: vec!["apparatus:default:asset-007".to_string()],
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
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
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
                "apparatus:default:bosma_7",
                "apparatus:default:asset-007",
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
                "apparatus":"apparatus:default:bosma_7",
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
                "apparatus":"apparatus:default:bosma_7",
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
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=apparatus%3Adefault%3Abosma_7&next_apparatus=apparatus%3Adefault%3Aasset-007&order_id=zakaz-wip-next&status=waiting",
            &lamin_token,
        ))
        .await
        .expect("list next apparatus wip");
    let listed_status = listed.status();
    let listed_body = json_body(listed).await;
    assert_eq!(listed_status, StatusCode::OK, "{listed_body:?}");
    assert_eq!(listed_body["batches"][0]["qr_payload"], qr_payload);
    assert_eq!(
        listed_body["batches"][0]["next_apparatus"],
        "apparatus:default:asset-007"
    );
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
                "apparatus:default:bosma_7".to_string(),
                "apparatus:default:asset-007".to_string(),
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
                "apparatus:default:bosma_7",
                "apparatus:default:asset-007",
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
                "apparatus":"apparatus:default:bosma_7",
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
                "apparatus":"apparatus:default:bosma_7",
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
                    "apparatus":"apparatus:default:asset-007",
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
                    "apparatus":"apparatus:default:asset-007",
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
    assert_eq!(completed_body["states"]["zakaz-wip-complete-qr"], "pending");
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
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
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
                "apparatus:default:bosma_7",
                "apparatus:default:asset-007",
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
                "apparatus":"apparatus:default:bosma_7",
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
                "apparatus":"apparatus:default:bosma_7",
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
            "/v1/mobile/admin/production-maps/wip-batches?apparatus=apparatus%3Adefault%3Aasset-007&status=waiting",
            &worker_token,
        ))
        .await
        .expect("unassigned wip");
    assert_eq!(unassigned.status(), StatusCode::FORBIDDEN);
}
