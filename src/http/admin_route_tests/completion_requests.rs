use super::*;

#[tokio::test]
async fn queue_complete_without_output_creates_admin_completion_request() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-zero-complete".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-zero-complete").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-zero-complete",
                "Zero complete order",
                "9311",
                "7 ta rangli pechat",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);
    provision_test_qolip(&router, &admin_token, "zakaz-zero-complete").await;

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-zero-complete",
                "action":"start"
            }"#, "zakaz-zero-complete"),
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let requested = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-zero-complete",
                "action":"complete",
                "completion_request_note":"Metraj va kg yo'q, brigader tekshirsin"
            }"#,
        ))
        .await
        .expect("complete request");
    let requested_status = requested.status();
    let requested_body = json_body(requested).await;
    assert_eq!(requested_status, StatusCode::OK, "{requested_body:?}");
    assert_eq!(
        requested_body["states"]["zakaz-zero-complete"],
        "in_progress"
    );
    assert!(requested_body["progress_batch"].is_null());
    assert_eq!(
        requested_body["completion_request"]["description"],
        "Metraj va kg yo'q, brigader tekshirsin"
    );
    assert_eq!(
        requested_body["completion_request"]["worker_ref"],
        "worker-zero-complete"
    );

    let listed = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completion-requests",
            &admin_token,
        ))
        .await
        .expect("list completion requests");
    let listed_status = listed.status();
    let listed_body = json_body(listed).await;
    assert_eq!(listed_status, StatusCode::OK, "{listed_body:?}");
    assert_eq!(
        listed_body["completion_requests"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        listed_body["completion_requests"][0]["order_id"],
        "zakaz-zero-complete"
    );
    assert_eq!(
        listed_body["completion_requests"][0]["description"],
        "Metraj va kg yo'q, brigader tekshirsin"
    );
}

#[tokio::test]
async fn queue_complete_with_zero_metric_requires_reason_and_creates_admin_request() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-zero-metric".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-zero-metric").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-zero-metric",
                "Zero metric order",
                "9312",
                "7 ta rangli pechat",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);
    provision_test_qolip(&router, &admin_token, "zakaz-zero-metric").await;

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-zero-metric",
                "action":"start"
            }"#, "zakaz-zero-metric"),
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let missing_reason = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-zero-metric",
                "action":"complete",
                "return_ink_kg":0,
                "total_waste":1,
                "finished_goods_kg":1,
                "finished_goods_meter":1
            }"#,
        ))
        .await
        .expect("complete without reason");
    let missing_reason_status = missing_reason.status();
    let missing_reason_body = json_body(missing_reason).await;
    assert_eq!(missing_reason_status, StatusCode::BAD_REQUEST);
    assert_eq!(
        missing_reason_body["error"],
        "zero_metric_explanation_required"
    );

    let requested = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-zero-metric",
                "action":"complete",
                "return_ink_kg":0,
                "total_waste":1,
                "finished_goods_kg":1,
                "finished_goods_meter":1,
                "completion_request_note":"Kraska qaytimi nol chiqdi, brigader tekshirsin"
            }"#,
        ))
        .await
        .expect("complete request");
    let requested_status = requested.status();
    let requested_body = json_body(requested).await;
    assert_eq!(requested_status, StatusCode::OK, "{requested_body:?}");
    assert_eq!(
        requested_body["states"]["zakaz-zero-metric"],
        "in_progress"
    );
    assert_eq!(
        requested_body["completion_request"]["zero_metric_codes"],
        serde_json::json!(["return_ink_kg"])
    );

    let listed = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completion-requests",
            &admin_token,
        ))
        .await
        .expect("list completion requests");
    let listed_status = listed.status();
    let listed_body = json_body(listed).await;
    assert_eq!(listed_status, StatusCode::OK, "{listed_body:?}");
    assert_eq!(
        listed_body["completion_requests"][0]["zero_metric_codes"],
        serde_json::json!(["return_ink_kg"])
    );
}

#[tokio::test]
async fn admin_approves_zero_output_completion_request_and_closes_order_with_issue_history() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-approve-complete".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-approve-complete").await;
    let router = build_router(state);

    for (order_id, number) in [
        ("zakaz-approve-zero", "9411"),
        ("zakaz-approve-next", "9412"),
    ] {
        let saved = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &admin_token,
                &pechat_order_map_json(
                    order_id,
                    "Approve zero order",
                    number,
                    "7 ta rangli pechat",
                ),
            ))
            .await
            .expect("save map");
        assert_eq!(saved.status(), StatusCode::OK);
    }
    let rule = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-rules",
            &admin_token,
            r#"{"apparatus":"7 ta rangli pechat","requires_material":false,"item_groups":["Kraska"]}"#,
        ))
        .await
        .expect("rule save");
    assert_eq!(rule.status(), StatusCode::OK);
    let assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &admin_token,
            r#"{
                "order_id":"zakaz-approve-zero",
                "barcode":"30AA"
            }"#,
        ))
        .await
        .expect("assign raw material");
    let assigned_status = assigned.status();
    let assigned_body = json_body(assigned).await;
    assert_eq!(assigned_status, StatusCode::OK, "{assigned_body:?}");
    let sequenced = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_ids":["zakaz-approve-zero","zakaz-approve-next"]
            }"#,
        ))
        .await
        .expect("sequence");
    assert_eq!(sequenced.status(), StatusCode::OK);
    provision_test_qolip(&router, &admin_token, "zakaz-approve-zero").await;
    provision_test_qolip(&router, &admin_token, "zakaz-approve-next").await;

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-approve-zero",
                "action":"start",
                "material_barcodes":["30AA"]
            }"#, "zakaz-approve-zero"),
        ))
        .await
        .expect("start");
    assert_eq!(started.status(), StatusCode::OK);

    let requested = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-approve-zero",
                "action":"complete",
                "completion_request_note":"kg va metraj kiritilmagan, buyurtma muammo bilan yopilsin"
            }"#,
        ))
        .await
        .expect("complete request");
    assert_eq!(requested.status(), StatusCode::OK);
    let requested_body = json_body(requested).await;
    let event_id = requested_body["completion_request"]["event_id"]
        .as_str()
        .expect("event id");

    let approved = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/completion-requests/decision",
            &admin_token,
            &format!(r#"{{"event_id":"{event_id}","decision":"approve"}}"#),
        ))
        .await
        .expect("approve");
    let approved_status = approved.status();
    let approved_body = json_body(approved).await;
    assert_eq!(approved_status, StatusCode::OK, "{approved_body:?}");
    assert_eq!(approved_body["states"]["zakaz-approve-zero"], "completed");
    assert_eq!(approved_body["decision"]["decision"], "approved");
    assert_eq!(approved_body["decision"]["message"], "Muammo bilan yopildi");
    assert_eq!(
        approved_body["order_status"]["order_status"],
        "completed_with_issue"
    );

    let assignments_after_approve = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments",
            &admin_token,
        ))
        .await
        .expect("assignments after approve");
    assert_eq!(assignments_after_approve.status(), StatusCode::OK);
    let assignments_body = json_body(assignments_after_approve).await;
    let approved_material = assignments_body
        .as_array()
        .expect("assignments array")
        .iter()
        .find(|item| item["order_id"] == "zakaz-approve-zero")
        .expect("approved order material");
    assert_eq!(approved_material["stock_status"], "consumed");
    assert_eq!(approved_material["reserved_order_id"], "zakaz-approve-zero");

    let requests_after = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/completion-requests",
            &admin_token,
        ))
        .await
        .expect("requests after approve");
    let requests_body = json_body(requests_after).await;
    assert_eq!(
        requests_body["completion_requests"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let closed = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/closed-orders",
            &admin_token,
        ))
        .await
        .expect("closed orders");
    let closed_body = json_body(closed).await;
    let order = &closed_body["closed_orders"][0];
    assert_eq!(order["order_id"], "zakaz-approve-zero");
    let issue_log = order["logs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|log| log["completed_with_issue"] == true)
        .expect("issue log");
    assert_eq!(issue_log["issue_note"], "Muammo bilan yopildi");

    let sequence_after_approve = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
        ))
        .await
        .expect("sequence after approve");
    let sequence_after_approve_status = sequence_after_approve.status();
    let sequence_after_approve_body = json_body(sequence_after_approve).await;
    assert_eq!(
        sequence_after_approve_status,
        StatusCode::OK,
        "{sequence_after_approve_body:?}"
    );
    assert_eq!(
        sequence_after_approve_body["order_statuses"]["zakaz-approve-zero"]["order_status"],
        "completed_with_issue"
    );

    let next_started = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(r#"{
                "apparatus":"7 ta rangli pechat",
                "order_id":"zakaz-approve-next",
                "action":"start"
            }"#, "zakaz-approve-next"),
        ))
        .await
        .expect("start next");
    assert_eq!(next_started.status(), StatusCode::OK);
}
