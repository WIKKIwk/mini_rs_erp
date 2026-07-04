use super::*;

#[tokio::test]
async fn raw_material_assignment_can_be_unlinked_before_start() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    let token = session(&state, PrincipalRole::Admin).await;
    let mut warehouse_events = state.warehouse_events.subscribe();
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json(
                "zakaz-raw-unlink",
                "Raw unlink",
                "8822",
                "7 ta rangli pechat - A",
            ),
        ))
        .await
        .expect("map save");
    assert_eq!(map.status(), StatusCode::OK);

    let rule = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-rules",
            &token,
            r#"{"apparatus":"7 ta rangli pechat - A","requires_material":true,"item_groups":["Kraska"]}"#,
        ))
        .await
        .expect("rule save");
    assert_eq!(rule.status(), StatusCode::OK);

    let assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{"order_id":"zakaz-raw-unlink","barcode":"30AA"}"#,
        ))
        .await
        .expect("assign");
    assert_eq!(assigned.status(), StatusCode::OK);
    let _assigned_event = warehouse_events.recv().await.expect("assigned event");

    let unlinked = router
        .clone()
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{"order_id":"zakaz-raw-unlink","barcode":"30AA"}"#,
        ))
        .await
        .expect("unlink assignment");
    let unlink_status = unlinked.status();
    let unlink_body = json_body(unlinked).await;
    assert_eq!(unlink_status, StatusCode::OK, "{unlink_body:?}");
    assert_eq!(unlink_body["ok"], true);
    assert_eq!(unlink_body["assignment"]["order_id"], "zakaz-raw-unlink");
    assert_eq!(unlink_body["assignment"]["barcode"], "30AA");
    let unlinked_event = warehouse_events.recv().await.expect("unlinked event");
    assert_eq!(unlinked_event.event, "warehouse.updated");
    assert_eq!(unlinked_event.warehouse, "Kalidor");
    assert_eq!(unlinked_event.reason, "raw_material_assignment_unlink");

    let assignments_after_unlink = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
        ))
        .await
        .expect("assignments after unlink");
    assert_eq!(assignments_after_unlink.status(), StatusCode::OK);
    let assignments_body = json_body(assignments_after_unlink).await;
    assert!(
        assignments_body
            .as_array()
            .expect("assignments array")
            .iter()
            .all(|item| item["order_id"] != "zakaz-raw-unlink")
    );

    let missing = router
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{"order_id":"zakaz-raw-unlink","barcode":"30AA"}"#,
        ))
        .await
        .expect("unlink missing assignment");
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing).await["error"],
        "raw_material_assignment_not_found"
    );
}

#[tokio::test]
async fn raw_material_assignment_unlink_rejects_started_stock() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-raw-unlink-locked".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat - A".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("aparatchi assignment");
    let token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-raw-unlink-locked").await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json(
                "zakaz-raw-unlink-locked",
                "Raw unlink locked",
                "8833",
                "7 ta rangli pechat - A",
            ),
        ))
        .await
        .expect("map save");
    assert_eq!(map.status(), StatusCode::OK);

    let rule = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-rules",
            &token,
            r#"{"apparatus":"7 ta rangli pechat - A","requires_material":true,"item_groups":["Kraska"]}"#,
        ))
        .await
        .expect("rule save");
    assert_eq!(rule.status(), StatusCode::OK);

    let assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{"order_id":"zakaz-raw-unlink-locked","barcode":"30AA"}"#,
        ))
        .await
        .expect("assign");
    assert_eq!(assigned.status(), StatusCode::OK);

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-raw-unlink-locked",
                "action":"start",
                "material_barcodes":["30AA"]
            }"#,
        ))
        .await
        .expect("start with material");
    assert_eq!(started.status(), StatusCode::OK);

    let locked = router
        .clone()
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{"order_id":"zakaz-raw-unlink-locked","barcode":"30AA"}"#,
        ))
        .await
        .expect("unlink locked assignment");
    assert_eq!(locked.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(locked).await["error"],
        "raw_material_assignment_locked"
    );

    let assignments_after_reject = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
        ))
        .await
        .expect("assignments after locked unlink");
    assert_eq!(assignments_after_reject.status(), StatusCode::OK);
    let assignments_body = json_body(assignments_after_reject).await;
    assert!(
        assignments_body
            .as_array()
            .expect("assignments array")
            .iter()
            .any(|item| {
                item["order_id"] == "zakaz-raw-unlink-locked"
                    && item["barcode"] == "30AA"
                    && item["stock_status"] == "in_use"
            })
    );
}
