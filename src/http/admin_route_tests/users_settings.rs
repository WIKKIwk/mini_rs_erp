use super::*;

#[tokio::test]
async fn admin_user_list_returns_merged_paged_users_with_role_labels() {
    let mut state = test_state();
    let role_store = Arc::new(MemoryRoleDefinitionStore::new());
    role_store
        .put_role_definition(RoleDefinition {
            id: "item_creator".to_string(),
            label: "Item yaratuvchi".to_string(),
            base_role: Some(PrincipalRole::Customer),
            capability_codes: vec!["catalog.item.create".to_string()],
            system: false,
        })
        .await
        .expect("role");
    role_store
        .put_role_assignment(RoleAssignment {
            principal_role: PrincipalRole::Customer,
            principal_ref: "CUST-001".to_string(),
            role_id: "item_creator".to_string(),
            assigned_apparatus: Vec::new(),
        })
        .await
        .expect("assignment");
    state.admin = state.admin.with_role_store(role_store);
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?q=customer&limit=2&offset=0",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["items"].as_array().expect("items").len(), 1);
    assert_eq!(value["items"][0]["id"], "customer:CUST-001");
    assert_eq!(value["items"][0]["source"], "customer");
    assert_eq!(value["items"][0]["entity_ref"], "CUST-001");
    assert_eq!(value["items"][0]["name"], "Customer One");
    assert_eq!(value["items"][0]["role_label"], "Item yaratuvchi");
    assert_eq!(value["has_more"], false);
}

#[tokio::test]
async fn admin_user_list_does_not_treat_customers_as_qolipchi() {
    let mut state = test_state();
    let role_store = Arc::new(MemoryRoleDefinitionStore::new());
    role_store
        .put_role_assignment(RoleAssignment {
            principal_role: PrincipalRole::Qolipchi,
            principal_ref: "CUST-001".to_string(),
            role_id: "qolipchi".to_string(),
            assigned_apparatus: Vec::new(),
        })
        .await
        .expect("assignment");
    state.admin = state
        .admin
        .with_role_store(role_store)
        .with_read_port(Arc::new(QolipchiCustomerLookupReadPort));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?q=qolipchi&limit=10&offset=0",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["items"].as_array().expect("items").len(), 0);
}

#[tokio::test]
async fn admin_settings_requires_admin_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Supplier).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/settings", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn admin_method_checks_happen_after_auth_like_go() {
    let state = test_state();
    let cases = [
        ("PATCH", "/v1/mobile/admin/settings"),
        ("POST", "/v1/mobile/admin/roles"),
        ("PATCH", "/v1/mobile/admin/workers"),
        ("POST", "/v1/mobile/admin/production-maps"),
        ("POST", "/v1/mobile/admin/role-assignments"),
        ("PATCH", "/v1/mobile/admin/suppliers"),
        ("POST", "/v1/mobile/admin/users/list"),
        ("POST", "/v1/mobile/admin/suppliers/list"),
        ("POST", "/v1/mobile/admin/suppliers/summary"),
        ("POST", "/v1/mobile/admin/suppliers/detail"),
        ("POST", "/v1/mobile/admin/suppliers/inactive"),
        ("POST", "/v1/mobile/admin/suppliers/items/assigned"),
        ("POST", "/v1/mobile/admin/suppliers/status"),
        ("POST", "/v1/mobile/admin/suppliers/phone"),
        ("POST", "/v1/mobile/admin/suppliers/items"),
        ("GET", "/v1/mobile/admin/suppliers/items/add"),
        ("GET", "/v1/mobile/admin/suppliers/items/remove"),
        ("GET", "/v1/mobile/admin/suppliers/code/regenerate"),
        ("GET", "/v1/mobile/admin/suppliers/remove"),
        ("GET", "/v1/mobile/admin/suppliers/restore"),
        ("PATCH", "/v1/mobile/admin/customers"),
        ("POST", "/v1/mobile/admin/customers/list"),
        ("POST", "/v1/mobile/admin/customers/detail"),
        ("POST", "/v1/mobile/admin/customers/phone"),
        ("GET", "/v1/mobile/admin/customers/code/regenerate"),
        ("GET", "/v1/mobile/admin/customers/items/add"),
        ("GET", "/v1/mobile/admin/customers/items/remove"),
        ("GET", "/v1/mobile/admin/customers/remove"),
        ("PATCH", "/v1/mobile/admin/items"),
        ("GET", "/v1/mobile/admin/items/bulk-move-group"),
        ("PATCH", "/v1/mobile/admin/item-groups"),
        ("POST", "/v1/mobile/admin/item-groups/tree"),
        ("POST", "/v1/mobile/admin/activity"),
        ("GET", "/v1/mobile/admin/werka/code/regenerate"),
    ];

    let supplier_token = session(&state, PrincipalRole::Supplier).await;
    let admin_token = session(&state, PrincipalRole::Admin).await;
    for (method, path) in cases {
        let unauthorized = build_router(state.clone())
            .oneshot(request(method, path, ""))
            .await
            .expect("response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(json_body(unauthorized).await["error"], "unauthorized");

        let forbidden = build_router(state.clone())
            .oneshot(request(method, path, &supplier_token))
            .await
            .expect("response");
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN, "{path}");
        assert_eq!(json_body(forbidden).await["error"], "forbidden");

        let method_not_allowed = build_router(state.clone())
            .oneshot(request(method, path, &admin_token))
            .await
            .expect("response");
        assert_eq!(
            method_not_allowed.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{path}"
        );
        assert_eq!(
            json_body(method_not_allowed).await["error"],
            "method not allowed"
        );
    }
}
