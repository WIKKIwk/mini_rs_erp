use super::*;

#[tokio::test]
async fn sales_creates_orders_edits_maps_and_reads_complete_work_map_without_queue_mutations() {
    let state = test_state();
    let sales = session_for(&state, PrincipalRole::SalesManager, "sales_manager-1").await;
    let app = build_router(state.clone());
    let map = serde_json::from_str::<serde_json::Value>(&pechat_order_map_json(
        "zakaz-7777",
        "Sales order",
        "7777",
        "apparatus:default:bosma_8",
    ))
    .unwrap();
    let body = serde_json::json!({"map":map,"template":{
        "name":"Sales product","product":"Sales product","customer":"Customer One",
        "frame_product_size_mm":635,"frame_count":1,"first_layer_material":"pet","first_layer_micron":"12"
    }}).to_string();
    let response = app
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &sales,
            &body,
        ))
        .await
        .unwrap();
    let status = response.status();
    let saved = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let mut edited = saved["saved"]["map"].clone();
    edited["nodes"][0]["x"] = 123.into();
    let response = app
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &sales,
            &edited.to_string(),
        ))
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(status, StatusCode::OK, "{}", json_body(response).await);
    for path in [
        "/v1/mobile/admin/production-maps?id=zakaz-7777",
        "/v1/mobile/admin/production-maps/sequence",
        "/v1/mobile/admin/production-maps/order-page?view=orders&limit=20",
        "/v1/mobile/admin/production-maps/sequence?detail=true&order_id=zakaz-7777",
        "/v1/mobile/admin/production-maps/wip-batches?order_id=zakaz-7777",
        "/v1/mobile/admin/calculate-materials",
        "/v1/mobile/admin/apparatus",
        "/v1/mobile/admin/apparatus/options",
        "/v1/mobile/admin/items/uoms",
    ] {
        let response = app
            .clone()
            .oneshot(request("GET", path, &sales))
            .await
            .unwrap();
        let status = response.status();
        assert_eq!(
            status,
            StatusCode::OK,
            "{path}: {}",
            json_body(response).await
        );
    }
    let before = state.production_maps.live_snapshot_shared().await.unwrap();
    for path in [
        "sequence",
        "queue-action",
        "move",
        "move-batch",
        "apparatus-transfer",
        "order-control",
        "temporary-pause",
        "re-pause",
        "reactivation",
        "reactivation/reclose",
        "completion-requests/decision",
        "paddons/create",
        "progress-qr/correct",
        "progress-qr/reprint",
    ] {
        for method in ["GET", "POST", "PUT", "DELETE"] {
            if path == "sequence" && method == "GET" {
                continue;
            }
            let response = app.clone().oneshot(request_with_body(method, &format!("/v1/mobile/admin/production-maps/{path}"), &sales, r#"{"order_id":"zakaz-7777","apparatus":"apparatus:default:bosma_8","action":"freeze"}"#)).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
        }
    }
    assert_eq!(
        state
            .production_maps
            .live_snapshot_shared()
            .await
            .unwrap()
            .queue_states,
        before.queue_states
    );
    assert_eq!(
        state
            .production_maps
            .live_snapshot_shared()
            .await
            .unwrap()
            .sequences,
        before.sequences
    );
}

#[tokio::test]
async fn sales_creates_only_customers_and_finished_products() {
    let mut state = test_state();
    let states = Arc::new(FakeAdminStatePort::new());
    states
        .put_state(
            "CUST-NEW",
            AdminState {
                custom_code: "30PRIVATE123".into(),
                ..AdminState::default()
            },
        )
        .await
        .unwrap();
    state.admin = state.admin.clone().with_state_port(states);
    let sales = session_for(&state, PrincipalRole::SalesManager, "sales_manager-2").await;
    let admin = session(&state, PrincipalRole::Admin).await;
    let app = build_router(state);
    let response = app
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/customers",
            &sales,
            r#"{"name":"Sales customer","phone":"+998119998877"}"#,
        ))
        .await
        .unwrap();
    let status = response.status();
    let created_customer = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{created_customer}");
    assert_eq!(created_customer["ref"], "CUST-NEW");
    assert_eq!(created_customer["name"], "Sales customer");
    let response = app
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/customers/detail?ref=CUST-NEW",
            &sales,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["code"], "");
    let response = app
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/customers/detail?ref=CUST-NEW",
            &admin,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        !json_body(response).await["code"]
            .as_str()
            .unwrap_or_default()
            .is_empty()
    );
    for role in [
        "supplier",
        "material_taminotchi",
        "admin",
        "sales_manager",
        "qolipchi",
    ] {
        let response = app
            .clone()
            .oneshot(request(
                "GET",
                &format!("/v1/mobile/admin/users/list?role={role}"),
                &sales,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "list {role}");
    }
    let response = app
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=customer",
            &sales,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    for path in [
        "suppliers",
        "system-users",
        "workers",
        "material-taminotchilar",
        "role-assignments",
        "roles",
        "customers/phone",
        "customers/remove",
        "customers/code/regenerate",
    ] {
        let response = app
            .clone()
            .oneshot(request_with_body(
                "POST",
                &format!("/v1/mobile/admin/{path}"),
                &sales,
                r#"{"name":"Forged staff","phone":"+998119998876","role":"admin"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "create {path}");
    }
    for group in ["Xomashyo", "Rulon", "Tayyor mahsulot raw"] {
        let body = serde_json::json!({"code":"FORGED","name":"Raw item","uom":"Kg","item_group":group,"customer_ref":"CUST-001"}).to_string();
        let response = app
            .clone()
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/admin/items",
                &sales,
                &body,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{group}");
    }
    let body = r#"{"code":"","name":"Finished sales item","uom":"Dona","item_group":"Tayyor mahsulot","customer_ref":"CUST-001"}"#;
    let response = app
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items",
            &sales,
            body,
        ))
        .await
        .unwrap();
    let status = response.status();
    let item = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{item}");
    assert_eq!(item["item_group"], "Tayyor mahsulot");
    let response = app
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/items/detail",
            &sales,
            r#"{"original_code":"ITEM-001","code":"ITEM-002","name":"Unauthorized edit"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn sales_order_creation_catalog_requests_pass_authorization() {
    let state = test_state();
    let sales = session_for(&state, PrincipalRole::SalesManager, "sales-catalog").await;
    let app = build_router(state);
    for path in [
        "/v1/mobile/admin/customers/detail?ref=CUST-001",
        "/v1/mobile/admin/items/detail?code=MULTI-ONE",
    ] {
        let response = app.clone().oneshot(request("GET", path, &sales)).await.unwrap();
        let status = response.status();
        let body = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        if path.contains("items/detail") {
            assert_eq!(body["customers"][0]["ref"], "CUST-001");
        }
    }
    let calculate = r#"{"product":"Sales product","kg":100,"frame_product_size_mm":500,"frame_count":1,"layers":[{"material":"pet","micron":"12"}]}"#;
    let response = app.clone().oneshot(request_with_body("POST", "/v1/mobile/calculate", &sales, calculate)).await.unwrap();
    let status = response.status();
    assert_eq!(status, StatusCode::OK, "calculate: {}", json_body(response).await);
    let response = app.clone().oneshot(request("GET", "/v1/mobile/admin/combos?item_code=COMBO-R1", &sales)).await.unwrap();
    let status = response.status();
    let body = json_body(response).await;
    // The fixture has no combo database; reaching its unavailable-store error
    // proves that sales passed both the scope and the combo-read authorization.
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "combo read: {body}");
    assert_eq!(body["error"], "combo_store_unavailable");
    let response = app.oneshot(request_with_body("POST", "/v1/mobile/admin/production-maps/auto-open", &sales, r#"{"template":{}}"#)).await.unwrap();
    let status = response.status();
    assert_ne!(status, StatusCode::FORBIDDEN, "auto-open: {}", json_body(response).await);
}
