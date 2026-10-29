use super::*;

#[tokio::test]
async fn brigader_personal_system_user_has_a_93_code_and_logs_in_to_its_own_role() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let app = build_router(state.clone());
    let response = app
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/system-users",
            &token,
            r#"{"role":"brigader","name":"Brigader One","phone":"+998907771122"}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let user = json_body(response).await;
    let id = user["id"].as_str().unwrap();
    assert!(id.starts_with("brigader_"));
    let response = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/v1/mobile/admin/system-users/code/regenerate?id={id}"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let issued = json_body(response).await;
    let code = issued["code"].as_str().unwrap();
    assert!(code.starts_with("93") && code.bytes().all(|byte| byte.is_ascii_digit()));
    let body = serde_json::json!({"phone":"+998907771122", "code":code}).to_string();
    let response = app
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            &body,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let login = json_body(response).await;
    assert_eq!(login["profile"]["role"], "brigader");
    assert_eq!(login["profile"]["ref"], id);
    let capabilities = login["capabilities"].as_array().unwrap();
    assert!(capabilities.contains(&serde_json::json!("brigader.access")));
    assert!(!capabilities.contains(&serde_json::json!("admin.access")));
}

#[tokio::test]
async fn brigader_reuses_admin_pages_order_creation_and_additional_settings() {
    let mut state = test_state();
    let directory = tempfile::tempdir().unwrap();
    state.mobile_releases = crate::core::mobile_release::MobileReleaseStore::new(directory.path());
    let token = session_for(&state, PrincipalRole::Brigader, "brigader-1").await;
    let app = build_router(state.clone());
    for path in [
        "admin/production-maps/sequence",
        "admin/production-maps/order-page?view=orders&limit=20",
        "admin/production-maps/wip-batches",
        "admin/production-maps/completion-requests",
        "admin/calculate-materials",
        "admin/apparatus",
        "admin/apparatus/options",
        "admin/items/uoms",
        "admin/roles",
        "admin/capabilities",
        "admin/role-assignments",
        "admin/workers",
        "admin/warehouses",
        "admin/factory-locations",
        "admin/item-groups",
        "admin/users/list?role=brigader",
        "admin/app-update/settings",
    ] {
        let response = app
            .clone()
            .oneshot(request("GET", &format!("/v1/mobile/{path}"), &token))
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
    let map: serde_json::Value = serde_json::from_str(&pechat_order_map_json(
        "zakaz-7788",
        "Brigader order",
        "7788",
        "apparatus:default:bosma_8",
    ))
    .unwrap();
    let body = serde_json::json!({"map":map,"template":{
        "name":"Brigader product","product":"Brigader product","customer":"Customer One",
        "frame_product_size_mm":635,"frame_count":1,"first_layer_material":"pet","first_layer_micron":"12"
    }}).to_string();
    let response = app
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        ))
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(status, StatusCode::OK, "{}", json_body(response).await);
    for (path, body) in [
        (
            "admin/app-update/settings",
            r#"{"mandatory_updates_enabled":true}"#,
        ),
        (
            "admin/production-maps/paddons/management-settings",
            r#"{"free_movement_enabled":true,"worker_visibility_enabled":true}"#,
        ),
    ] {
        let response = app
            .clone()
            .oneshot(request_with_body(
                "PUT",
                &format!("/v1/mobile/{path}"),
                &token,
                body,
            ))
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
}

#[tokio::test]
async fn brigader_cannot_escalate_its_fixed_role_or_enter_unlisted_admin_pages() {
    let mut state = test_state();
    let roles = Arc::new(MemoryRoleDefinitionStore::new());
    roles
        .put_role_definition(RoleDefinition {
            id: "forged-admin".into(),
            label: "Forged".into(),
            base_role: None,
            system: false,
            capability_codes: vec![
                "brigader.access".into(),
                "admin.access".into(),
                "role.capability.manage".into(),
                "admin.settings.manage".into(),
            ],
        })
        .await
        .unwrap();
    roles
        .put_role_assignment(RoleAssignment {
            principal_role: PrincipalRole::Brigader,
            principal_ref: "brigader-1".into(),
            role_id: "forged-admin".into(),
            assigned_apparatus: vec![],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    state.admin = state.admin.clone().with_role_store(roles);
    let token = session_for(&state, PrincipalRole::Brigader, "brigader-1").await;
    let principal = state.sessions.get(&token).await.unwrap();
    assert_eq!(
        state.admin.principal_capability_codes(&principal).await,
        vec!["brigader.access"]
    );
    assert!(
        !state
            .admin
            .principal_has_capability(&principal, crate::core::authz::Capability::AdminAccess)
            .await
    );
    let app = build_router(state);
    for path in [
        "admin/settings",
        "admin/telegram/settings",
        "admin/push-config",
        "admin/system/monitor",
        "admin/system/backups",
        "admin/emergency-reset/orders",
        "admin/training/apparatus",
    ] {
        for method in ["GET", "POST", "PUT", "DELETE"] {
            let response = app
                .clone()
                .oneshot(request_with_body(
                    method,
                    &format!("/v1/mobile/{path}"),
                    &token,
                    "{}",
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
        }
    }
}

#[tokio::test]
async fn brigader_user_creation_assigns_staff_system_roles_and_safe_custom_roles() {
    let mut state = test_state();
    let roles = Arc::new(MemoryRoleDefinitionStore::new());
    for (id, capability) in [
        ("catalog-reader", "catalog.item.read"),
        ("system-admin", "admin.access"),
    ] {
        roles
            .put_role_definition(RoleDefinition {
                id: id.into(),
                label: id.into(),
                base_role: None,
                system: false,
                capability_codes: vec![capability.into()],
            })
            .await
            .unwrap();
    }
    state.admin = state.admin.clone().with_role_store(roles);
    let token = session_for(&state, PrincipalRole::Brigader, "brigader-1").await;
    let app = build_router(state);
    for (role, principal_role, expected) in [
        ("catalog-reader", "supplier", StatusCode::OK),
        ("aparatchi", "aparatchi", StatusCode::OK),
        ("admin", "admin", StatusCode::FORBIDDEN),
        ("system-admin", "supplier", StatusCode::FORBIDDEN),
        ("catalog-reader", "admin", StatusCode::FORBIDDEN),
    ] {
        let body = serde_json::json!({"principal_role":principal_role,
            "principal_ref":if principal_role == "aparatchi" {"CUST-001"} else {"SUP-001"}, "role_id":role})
        .to_string();
        let response = app
            .clone()
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/role-assignments",
                &token,
                &body,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{principal_role} {role}");
    }
    let response = app
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/roles",
            &token,
            r#"{"id":"new","label":"new","capability_codes":["admin.access"]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn brigader_query_token_cannot_bypass_excluded_websocket_monitor_scope() {
    let state = test_state();
    let brigader = session_for(&state, PrincipalRole::Brigader, "brigader-live").await;
    let admin = session(&state, PrincipalRole::Admin).await;
    let app = build_router(state);
    for (bearer, query) in [
        ("", brigader.as_str()),
        (admin.as_str(), brigader.as_str()),
        (brigader.as_str(), admin.as_str()),
    ] {
        let response = app
            .clone()
            .oneshot(request(
                "GET",
                &format!("/v1/mobile/admin/system/monitor/live?token={query}"),
                bearer,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
