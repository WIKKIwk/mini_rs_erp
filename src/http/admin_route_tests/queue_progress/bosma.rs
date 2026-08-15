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
            assigned_item_groups: Vec::new(),
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

    provision_test_qolip(&router, &admin_token, "zakaz-bosma-complete").await;

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                r#"{
                "apparatus":"7 ta rangli bosma",
                "order_id":"zakaz-bosma-complete",
                "action":"start"
            }"#,
                "zakaz-bosma-complete",
            ),
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
                "action":"complete",
                "returned_paint_items":[
                    {"usage":"rasxot","category":"colors","name":"Oq","values":{"Mix":3,"Oq":1,"Qora":0}},
                    {"usage":"astatka","category":"colors","name":"Oq","values":{"Mix":1,"Oq":0,"Qora":0}}
                ]
            }"#,
        ))
        .await
        .expect("complete without metrics");
    assert_eq!(missing_metrics.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_metrics).await["error"],
        "bosma_completion_metrics_required"
    );

    let invalid_astatka = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli bosma",
                "order_id":"zakaz-bosma-complete",
                "action":"complete",
                "returned_paint_items":[
                    {"usage":"rasxot","category":"colors","name":"Oq","values":{"Mix":1,"Oq":1,"Qora":0}},
                    {"usage":"astatka","category":"colors","name":"Oq","values":{"Mix":2,"Oq":0,"Qora":0}}
                ],
                "total_waste":2.5,
                "finished_goods_kg":18.75,
                "finished_goods_meter":125.5,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("complete with invalid astatka");
    assert_eq!(invalid_astatka.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid_astatka).await["error"],
        "returned_paint_astatka_exceeds_rasxot"
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
                "returned_paint_items":[
                    {"usage":"rasxot","category":"colors","name":"Oq","values":{"Mix":9,"Oq":0,"Qora":0}},
                    {"usage":"astatka","category":"colors","name":"Oq","values":{"Mix":0.75,"Oq":0.25}},
                    {"usage":"astatka","category":"solvents","name":"Spirtlar","values":{"Etil":0.25}}
                ],
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
    assert_eq!(
        completed_body["progress_batch"]["payload_json"]["total_waste_uom"],
        "kg"
    );
    assert_eq!(completed_body["progress_batch"]["finished_goods_kg"], 18.75);
    assert_eq!(
        completed_body["progress_batch"]["finished_goods_meter"],
        125.5
    );
    assert_eq!(
        completed_body["progress_event"]["finished_goods_meter"],
        125.5
    );
    assert_eq!(
        completed_body["progress_event"]["payload_json"]["total_waste_uom"],
        "kg"
    );
    wait_for_progress_print_request_count(&print_requests, 1).await;
    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 1);
    assert_eq!(printed[0].gross_qty, 18.75);
    assert_eq!(printed[0].qty, Some(125.5));
    assert_eq!(printed[0].unit, "kg");
    assert_eq!(printed[0].progress_unit, "m");
}

#[tokio::test]
async fn bosma_pause_does_not_persist_completion_metrics() {
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
            assigned_item_groups: Vec::new(),
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

    provision_test_qolip(&router, &admin_token, "zakaz-bosma-pause").await;

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                r#"{
                "apparatus":"8 ta rangli bosma",
                "order_id":"zakaz-bosma-pause",
                "action":"start"
            }"#,
                "zakaz-bosma-pause",
            ),
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
                "total_waste":2.5,
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("pause with completion metrics ignored");
    let paused_status = paused.status();
    let paused_body = json_body(paused).await;
    assert_eq!(paused_status, StatusCode::OK, "{paused_body:?}");
    assert_eq!(paused_body["progress_batch"]["status"], "paused");
    assert!(paused_body["progress_batch"]["return_ink_kg"].is_null());
    assert!(paused_body["progress_batch"]["total_waste"].is_null());
    assert_eq!(paused_body["progress_batch"]["finished_goods_kg"], 12.0);
    assert_eq!(paused_body["progress_batch"]["finished_goods_meter"], 80.0);
}

#[tokio::test]
async fn bosma_worker_issue_freezes_order_without_paint_report_or_completion_metrics() {
    let mut state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-bosma-freeze-issue".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli bosma".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-bosma-freeze-issue").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-bosma-freeze-issue",
                "Bosma issue order",
                "9323",
                "7 ta rangli bosma",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);
    provision_test_qolip(&router, &admin_token, "zakaz-bosma-freeze-issue").await;

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                r#"{
                    "apparatus":"7 ta rangli bosma",
                    "order_id":"zakaz-bosma-freeze-issue",
                    "action":"start"
                }"#,
                "zakaz-bosma-freeze-issue",
            ),
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let admin_issue = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &admin_token,
            r#"{
                "apparatus":"7 ta rangli bosma",
                "order_id":"zakaz-bosma-freeze-issue",
                "action":"freeze",
                "freeze_with_issue":true,
                "issue_note":"Admin must not issue a worker freeze"
            }"#,
        ))
        .await
        .expect("admin issue attempt");
    assert_eq!(admin_issue.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(admin_issue).await["error"], "forbidden");

    let issue = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli bosma",
                "order_id":"zakaz-bosma-freeze-issue",
                "action":"freeze",
                "freeze_with_issue":true,
                "issue_note":"Bosma rang chiqishi notekis"
            }"#,
        ))
        .await
        .expect("issue pause");
    let issue_status = issue.status();
    let issue_body = json_body(issue).await;
    assert_eq!(issue_status, StatusCode::OK, "{issue_body:?}");
    assert_eq!(issue_body["states"]["zakaz-bosma-freeze-issue"], "frozen");
    assert_eq!(issue_body["order_status"]["order_status"], "frozen");
    assert_eq!(issue_body["order_control"]["state"], "frozen");
    assert_eq!(issue_body["order_control"]["freeze_request"]["status"], "frozen");
    assert!(issue_body["completion_request"].is_null());
    assert!(issue_body["progress_batch"].is_null());
    assert!(issue_body["print"].is_null());
    assert_eq!(issue_body["prints"].as_array().map(Vec::len), Some(0));

    let resume_while_frozen = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli bosma",
                "order_id":"zakaz-bosma-freeze-issue",
                "action":"resume"
            }"#,
        ))
        .await
        .expect("resume while frozen");
    assert_eq!(resume_while_frozen.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(resume_while_frozen).await["error"],
        "order_frozen"
    );

    let complete_while_frozen = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli bosma",
                "order_id":"zakaz-bosma-freeze-issue",
                "action":"complete",
                "returned_paint_items":[
                    {"usage":"rasxot","category":"colors","name":"Oq","values":{"Mix":3,"Oq":1,"Qora":0}},
                    {"usage":"astatka","category":"colors","name":"Oq","values":{"Mix":1,"Oq":0,"Qora":0}}
                ],
                "return_ink_kg":1,
                "total_waste":1,
                "finished_goods_kg":1,
                "finished_goods_meter":1
            }"#,
        ))
        .await
        .expect("complete while frozen");
    assert_eq!(complete_while_frozen.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(complete_while_frozen).await["error"],
        "order_frozen"
    );

    let unfreeze = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/order-control",
            &admin_token,
            r#"{
                "order_id":"zakaz-bosma-freeze-issue",
                "action":"unfreeze"
            }"#,
        ))
        .await
        .expect("unfreeze");
    let unfreeze_status = unfreeze.status();
    let unfreeze_body = json_body(unfreeze).await;
    assert_eq!(unfreeze_status, StatusCode::OK, "{unfreeze_body:?}");
    assert_eq!(unfreeze_body["control"]["state"], "active");

    let resumed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli bosma",
                "order_id":"zakaz-bosma-freeze-issue",
                "action":"resume"
            }"#,
        ))
        .await
        .expect("resume after unfreeze");
    let resumed_status = resumed.status();
    let resumed_body = json_body(resumed).await;
    assert_eq!(resumed_status, StatusCode::OK, "{resumed_body:?}");
    assert_eq!(resumed_body["states"]["zakaz-bosma-freeze-issue"], "in_progress");

    let ordinary_complete_without_report = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli bosma",
                "order_id":"zakaz-bosma-freeze-issue",
                "action":"complete"
            }"#,
        ))
        .await
        .expect("ordinary completion validation");
    assert_eq!(ordinary_complete_without_report.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(ordinary_complete_without_report).await["error"],
        "returned_paint_minimum_three_fields_or_image_only"
    );
}

#[tokio::test]
async fn bosma_can_complete_with_an_image_only_returned_paint_report() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-bosma-image-only".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli bosma".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-bosma-image-only").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-bosma-image-only",
                "Rasmli Bosma order",
                "8963",
                "7 ta rangli bosma",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    provision_test_qolip(&router, &admin_token, "zakaz-bosma-image-only").await;

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                r#"{
                    "apparatus":"7 ta rangli bosma",
                    "order_id":"zakaz-bosma-image-only",
                    "action":"start"
                }"#,
                "zakaz-bosma-image-only",
            ),
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let upload = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(
                    "/v1/mobile/returned-paint/images?order_id=zakaz-bosma-image-only&apparatus=7%20ta%20rangli%20bosma",
                )
                .header(header::AUTHORIZATION, format!("Bearer {worker_token}"))
                .header(header::CONTENT_TYPE, "image/jpeg")
                .header("x-file-name", "8963-qoldiq.jpg")
                .body(Body::from(b"image-only-returned-paint".to_vec()))
                .expect("upload request"),
        )
        .await
        .expect("upload response");
    let upload_status = upload.status();
    let upload_body = json_body(upload).await;
    assert_eq!(upload_status, StatusCode::OK, "{upload_body}");
    let image_id = upload_body["image"]["image_id"].as_str().expect("image id");

    let completed = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"7 ta rangli bosma",
                    "order_id":"zakaz-bosma-image-only",
                    "action":"complete",
                    "returned_paint_image_id":"{image_id}",
                    "returned_paint_items":[],
                    "total_waste":2.5,
                    "finished_goods_kg":18.75,
                    "finished_goods_meter":125.5
                }}"#
            ),
        ))
        .await
        .expect("image-only complete");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body}");
    assert_eq!(
        completed_body["states"]["zakaz-bosma-image-only"],
        "completed"
    );
    assert!(completed_body["progress_batch"]["return_ink_kg"].is_null());
    assert_eq!(completed_body["progress_batch"]["total_waste"], 2.5);
    assert_eq!(completed_body["progress_batch"]["finished_goods_kg"], 18.75);
    assert_eq!(
        completed_body["progress_batch"]["finished_goods_meter"],
        125.5
    );
}
