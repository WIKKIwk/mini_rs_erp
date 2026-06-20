use super::*;

#[tokio::test]
async fn queue_start_rejects_raw_material_stock_reserved_for_other_order() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store.clone());
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-raw-reserved".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat - A".to_string()],
        })
        .await
        .expect("aparatchi assignment");
    let token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-raw-reserved").await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json(
                "zakaz-raw-reserved",
                "Raw reserved",
                "8812",
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
            r#"{
                "order_id":"zakaz-raw-reserved",
                "barcode":"30AA"
            }"#,
        ))
        .await
        .expect("assign");
    assert_eq!(assigned.status(), StatusCode::OK);
    material_store
        .set_stock_status("30AA", "in_use", "zakaz-other")
        .await;

    let start = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-raw-reserved",
                "action":"start",
                "material_barcodes":["30AA"]
            }"#,
        ))
        .await
        .expect("queue action with reserved stock");
    let start_status = start.status();
    let start_body = json_body(start).await;
    assert_eq!(start_status, StatusCode::BAD_REQUEST, "{start_body:?}");
    assert_eq!(start_body["error"], "raw_material_stock_unavailable");

    let stock = material_store
        .raw_material_stock_by_barcode("30AA")
        .await
        .expect("stock lookup")
        .expect("stock");
    assert_eq!(stock.status, "in_use");
    assert_eq!(stock.reserved_order_id, "zakaz-other");
}

#[tokio::test]
async fn raw_material_assignment_checks_rulon_size_for_pechat_orders() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    material_store
        .insert_stock("30R980", "ROLL-980", "CPP 980/35", 10.0)
        .await;
    material_store
        .insert_stock("30R1000", "ROLL-1000", "CPP 1000/35", 11.0)
        .await;
    material_store
        .insert_stock("30R1020", "ROLL-1020", "CPP 1020/35", 9.0)
        .await;
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json_with_dims(
                "zakaz-rulon-size",
                "Rulon size",
                "8813",
                "7 ta rangli pechat - A",
                7.0,
                985.0,
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
            r#"{"apparatus":"7 ta rangli pechat - A","requires_material":true,"item_groups":["Rulon"]}"#,
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
            r#"{
                "order_id":"zakaz-rulon-size",
                "barcode":"30R1000",
                "item_code":"INK-BLACK",
                "item_group":"Kraska"
            }"#,
        ))
        .await
        .expect("assign matching rulon");
    let assigned_status = assigned.status();
    let assigned_body = json_body(assigned).await;
    assert_eq!(assigned_status, StatusCode::OK, "{assigned_body:?}");
    assert_eq!(assigned_body["item_code"], "ROLL-1000");
    assert_eq!(assigned_body["item_name"], "CPP 1000/35");
    assert_eq!(assigned_body["item_group"], "Rulon eni");

    let undersized = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{
                "order_id":"zakaz-rulon-size",
                "barcode":"30R980"
            }"#,
        ))
        .await
        .expect("assign undersized rulon");
    assert_eq!(undersized.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(undersized).await["error"],
        "raw_material_roll_size_mismatch"
    );

    let oversized = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{
                "order_id":"zakaz-rulon-size",
                "barcode":"30R1020"
            }"#,
        ))
        .await
        .expect("assign oversized rulon");
    assert_eq!(oversized.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(oversized).await["error"],
        "raw_material_roll_size_mismatch"
    );
}

#[tokio::test]
async fn admin_raw_material_stock_lists_new_stock_model() {
    let mut state = test_state();
    state.gscale =
        GscaleService::new().with_receipt_store(Arc::new(RawMaterialStockLookup::default()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-stock?warehouse=Kalidor",
            &token,
        ))
        .await
        .expect("raw stock list");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body[0]["warehouse"], "Kalidor");
    assert_eq!(body[0]["item_code"], "INK-BLACK");
    assert_eq!(body[0]["barcode"], "30AA");
    assert_eq!(body[0]["qty"], 12.0);
    assert_eq!(body[0]["status"], "available");
}
