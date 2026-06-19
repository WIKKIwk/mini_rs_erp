use super::*;

#[tokio::test]
async fn admin_settings_ignores_state_read_failure_like_go() {
    let mut state = test_state();
    let admin_port = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(admin_port.clone())
        .with_write_port(admin_port)
        .with_state_port(Arc::new(FailingAdminStatePort));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/settings", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["werka_code_locked"], false);
    assert_eq!(value["werka_code_retry_after_sec"], 0);
}

#[tokio::test]
async fn admin_suppliers_summary_failure_uses_go_error_text() {
    let mut state = test_state();
    let admin_port = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(admin_port.clone())
        .with_write_port(admin_port)
        .with_state_port(Arc::new(FailingAdminStatePort));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/suppliers", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "supplier summary failed"
    );
}

#[tokio::test]
async fn admin_settings_put_updates_default_uom_like_go() {
    let mut state = test_state();
    let admin_port = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(admin_port.clone())
        .with_write_port(admin_port)
        .with_state_port(Arc::new(FakeAdminStatePort::new()))
        .with_auth_config_sink(Arc::new(state.auth.clone()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/settings",
            &token,
            r#"{
                "default_target_warehouse":"Stores - NEW",
                "default_uom":"",
                "werka_phone":"+998881111111",
                "werka_name":"New Werka",
                "werka_code":"20NEW",
                "werka_code_locked":false,
                "werka_code_retry_after_sec":0,
                "admin_phone":"+998882222222",
                "admin_name":"New Admin"
            }"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["default_target_warehouse"], "Stores - NEW");
    assert_eq!(value["default_uom"], "Kg");
}

#[tokio::test]
async fn admin_suppliers_page_filters_removed_and_counts_blocked_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/suppliers", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["summary"]["total_suppliers"], 3);
    assert_eq!(value["summary"]["active_suppliers"], 1);
    assert_eq!(value["summary"]["blocked_suppliers"], 2);
    assert_eq!(value["suppliers"].as_array().expect("suppliers").len(), 2);
    assert_eq!(value["suppliers"][0]["ref"], "SUP-001");
    assert_eq!(value["suppliers"][0]["assigned_item_count"], 2);
    assert_eq!(value["customers"][0]["ref"], "CUST-001");
}

#[tokio::test]
async fn admin_supplier_detail_requires_ref_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/suppliers/detail", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "ref is required");
}

#[tokio::test]
async fn admin_supplier_detail_returns_assigned_items_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/suppliers/detail?ref=SUP-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["ref"], "SUP-001");
    assert_eq!(value["code"], "10CUSTOM");
    assert_eq!(value["assigned_items"][0]["code"], "ITEM-001");
}

#[tokio::test]
async fn admin_supplier_detail_uses_permission_fallback_like_go() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(AssignedItemsErrorReadPort::permission()))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/suppliers/detail?ref=SUP-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["assigned_items"].as_array().expect("items").len(), 2);
    assert_eq!(value["assigned_items"][0]["code"], "ITEM-001");
}

#[tokio::test]
async fn admin_supplier_detail_does_not_fallback_on_non_permission_error() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(AssignedItemsErrorReadPort::lookup_failed()))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/suppliers/detail?ref=SUP-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"], "supplier detail failed");
}

#[tokio::test]
async fn admin_assigned_supplier_items_permission_without_cache_is_empty_like_go() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(AssignedItemsErrorReadPort::permission()))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/suppliers/items/assigned?ref=SUP-002",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        json_body(response)
            .await
            .as_array()
            .expect("items")
            .is_empty()
    );
}

#[tokio::test]
async fn admin_customers_and_items_read_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let customers = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/customers/list", &token))
        .await
        .expect("response");
    assert_eq!(customers.status(), StatusCode::OK);
    assert_eq!(json_body(customers).await[0]["ref"], "CUST-001");

    let items = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/items?q=rice&limit=5&offset=1",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(items.status(), StatusCode::OK);
    assert_eq!(json_body(items).await[0]["item_group"], "Products");

    let group_items = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/items?group=Products&limit=5",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(group_items.status(), StatusCode::OK);
    assert_eq!(json_body(group_items).await[0]["code"], "ITEM-001");

    let groups = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/item-groups", &token))
        .await
        .expect("response");
    assert_eq!(groups.status(), StatusCode::OK);
    assert_eq!(json_body(groups).await[0], "All Item Groups");
}

#[tokio::test]
async fn admin_customer_list_passes_query_to_read_port() {
    let mut state = test_state();
    let seen_query = Arc::new(Mutex::new(String::new()));
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(QueryCaptureReadPort {
            seen_query: seen_query.clone(),
        }))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/customers/list?q=ali&limit=5&offset=2",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await[0]["ref"], "CUST-QUERY");
    assert_eq!(&*seen_query.lock().await, "ali");
}
