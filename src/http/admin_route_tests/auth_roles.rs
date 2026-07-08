use super::*;

#[tokio::test]
async fn admin_settings_returns_config_shape_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/settings", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["default_uom"], "Kg");
    assert_eq!(value["werka_name"], "Werka");
    assert_eq!(value["admin_name"], "Admin");
}

#[tokio::test]
async fn admin_capabilities_returns_role_builder_catalog() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let supplier_token = session(&state, PrincipalRole::Supplier).await;

    let forbidden = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/capabilities",
            &supplier_token,
        ))
        .await
        .expect("response");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(forbidden).await["error"], "forbidden");

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/capabilities",
            &admin_token,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    let items = value.as_array().expect("catalog array");

    assert!(items.iter().any(|item| item["code"] == "admin.access"));
    assert!(
        items
            .iter()
            .any(|item| item["code"] == "gscale.catalog.read")
    );
    assert!(items.iter().any(|item| {
        item["default_roles"]
            .as_array()
            .expect("roles")
            .contains(&serde_json::json!("werka"))
    }));
}

#[tokio::test]
async fn admin_roles_can_list_system_roles_and_save_custom_packages() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let supplier_token = session(&state, PrincipalRole::Supplier).await;

    let forbidden = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/roles", &supplier_token))
        .await
        .expect("response");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(forbidden).await["error"], "forbidden");

    let response = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/roles", &admin_token))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    let roles = value.as_array().expect("roles array");
    assert!(roles.iter().any(|role| role["id"] == "admin"));
    assert!(roles.iter().any(|role| role["id"] == "werka"));

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/roles",
            &admin_token,
            r#"{
                "id":"scale_operator",
                "label":"Scale operator",
                "capability_codes":[
                    "gscale.catalog.read",
                    "gscale.print",
                    "rps.batch.manage",
                    "gscale.print"
                ]
            }"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let saved = json_body(response).await;
    assert_eq!(saved["id"], "scale_operator");
    assert_eq!(saved["system"], false);
    assert!(saved.get("base_role").is_none());
    assert_eq!(
        saved["capability_codes"],
        serde_json::json!(["gscale.catalog.read", "gscale.print", "rps.batch.manage"])
    );

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/roles", &admin_token))
        .await
        .expect("response");
    let value = json_body(response).await;
    assert!(value.as_array().expect("roles").iter().any(|role| {
        role["id"] == "scale_operator" && role["capability_codes"][0] == "gscale.catalog.read"
    }));
}

#[tokio::test]
async fn admin_roles_hide_legacy_custom_roles_that_conflict_with_system_roles() {
    let mut state = test_state();
    let role_store = Arc::new(MemoryRoleDefinitionStore::new());
    role_store
        .put_role_definition(RoleDefinition {
            id: "aparatchi".to_string(),
            label: "Custom aparatchi".to_string(),
            base_role: None,
            capability_codes: vec!["catalog.item.read".to_string()],
            system: false,
        })
        .await
        .expect("put legacy role");
    state.admin = state.admin.with_role_store(role_store);
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/roles", &admin_token))
        .await
        .expect("roles response");

    assert_eq!(response.status(), StatusCode::OK);
    let roles = json_body(response).await;
    let aparatchi_roles: Vec<_> = roles
        .as_array()
        .expect("roles")
        .iter()
        .filter(|role| role["id"] == "aparatchi")
        .collect();
    assert_eq!(aparatchi_roles.len(), 1, "{roles}");
    assert_eq!(aparatchi_roles[0]["label"], "Aparatchi");
    assert_eq!(aparatchi_roles[0]["system"], true);
}

#[tokio::test]
async fn admin_rejects_material_system_role_on_customer_directory_ref() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &admin_token,
            r#"{
                "principal_role":"material_taminotchi",
                "principal_ref":"CUST-001",
                "role_id":"material_taminotchi",
                "assigned_item_groups":["Kraska"]
            }"#,
        ))
        .await
        .expect("assignment response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn admin_creates_material_taminotchi_with_seventy_prefix_code() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/material-taminotchilar",
            &admin_token,
            r#"{
                "name":"Materialchi",
                "phone":"110000070",
                "assigned_item_groups":["Kraska"]
            }"#,
        ))
        .await
        .expect("create response");

    assert_eq!(response.status(), StatusCode::OK);
    let material = json_body(response).await;
    assert!(material["ref"].as_str().expect("ref").starts_with("MAT-"));
    assert!(material["code"].as_str().expect("code").starts_with("70"));
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        material["ref"].as_str().expect("ref"),
        "Kalidor",
    )
    .await;

    let login = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            &format!(
                r#"{{"phone":"110000070","code":"{}"}}"#,
                material["code"].as_str().expect("code")
            ),
        ))
        .await
        .expect("login response");

    assert_eq!(login.status(), StatusCode::OK);
    let login = json_body(login).await;
    assert_eq!(login["profile"]["role"], "material_taminotchi");
    assert_eq!(login["assigned_item_groups"], serde_json::json!(["Kraska"]));
    assert_eq!(login["assigned_warehouses"], serde_json::json!(["Kalidor"]));
}

#[tokio::test]
async fn admin_role_assignment_limits_runtime_capabilities() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/roles",
            &admin_token,
            r#"{
                "id":"catalog_only",
                "label":"Catalog only",
                "capability_codes":["gscale.catalog.read"]
            }"#,
        ))
        .await
        .expect("role response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &admin_token,
            r#"{
                "principal_role":"werka",
                "principal_ref":"werka",
                "role_id":"catalog_only"
            }"#,
        ))
        .await
        .expect("assignment response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["role_id"], "catalog_only");

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &admin_token,
            r#"{
                "principal_role":"supplier",
                "principal_ref":"SUP-001",
                "role_id":"catalog_only"
            }"#,
        ))
        .await
        .expect("supplier assignment response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["role_id"], "catalog_only");

    let werka_token = session_for(&state, PrincipalRole::Werka, "werka").await;
    let response = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/gscale/items?limit=1",
            &werka_token,
        ))
        .await
        .expect("gscale items response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state.clone())
        .oneshot(request("POST", "/v1/mobile/werka/summary", &werka_token))
        .await
        .expect("werka summary response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/rps/batch/start",
            &werka_token,
            r#"{"item_code":"ITEM-001","warehouse":"Stores - CH"}"#,
        ))
        .await
        .expect("rps start response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn login_returns_effective_capabilities_for_assigned_custom_role() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/roles",
            &admin_token,
            r#"{
                "id":"scale_only",
                "label":"Scale only",
                "capability_codes":["gscale.catalog.read","gscale.print","rps.batch.manage"]
            }"#,
        ))
        .await
        .expect("role response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &admin_token,
            r#"{
                "principal_role":"werka",
                "principal_ref":"werka",
                "role_id":"scale_only",
                "assigned_apparatus":["Paket aparat"]
            }"#,
        ))
        .await
        .expect("assignment response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            r#"{"phone":"+99888862440","code":"20ABCDEF1234"}"#,
        ))
        .await
        .expect("login response");
    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(
        value["capabilities"],
        serde_json::json!(["gscale.catalog.read", "gscale.print", "rps.batch.manage"])
    );
    assert_eq!(
        value["assigned_apparatus"],
        serde_json::json!(["Paket aparat"])
    );
}
