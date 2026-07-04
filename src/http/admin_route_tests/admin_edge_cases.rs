use super::*;

#[tokio::test]
async fn admin_customer_detail_errors_are_500_like_go() {
    let mut state = test_state();
    let failing_read_port = Arc::new(CustomerItemsFailReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(failing_read_port)
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/customers/detail?ref=CUST-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"], "customer detail failed");
}

#[tokio::test]
async fn admin_customer_code_regenerate_cooldown_is_500_like_go() {
    let mut state = test_state();
    let admin_port = Arc::new(FakeAdminReadPort);
    state.admin = AdminService::new(&state.config)
        .with_read_port(admin_port.clone())
        .with_write_port(admin_port)
        .with_state_port(Arc::new(LockedCustomerStatePort));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/customers/code/regenerate?ref=CUST-001",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "customer code regenerate failed"
    );
}

#[tokio::test]
async fn admin_supplier_phone_not_found_is_404_like_go() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(FakeAdminReadPort))
        .with_write_port(Arc::new(MissingSupplierWritePort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/suppliers/phone?ref=SUP-MISSING",
            &token,
            r#"{"phone":"+998901111111"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(response).await["error"], "supplier not found");
}

#[tokio::test]
async fn admin_supplier_phone_skips_write_for_removed_supplier_like_go() {
    let mut state = test_state();
    let writes = Arc::new(CountingSupplierWritePort::default());
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(FakeAdminReadPort))
        .with_write_port(writes.clone())
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/suppliers/phone?ref=SUP-003",
            &token,
            r#"{"phone":"+998901111111"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(response).await["error"], "supplier not found");
    assert_eq!(writes.supplier_phone_updates.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn admin_supplier_items_invalid_item_is_500_like_go() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(MissingItemsReadPort))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/suppliers/items?ref=SUP-001",
            &token,
            r#"{"item_codes":["ITEM-MISSING"]}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "supplier items update failed"
    );
}

#[tokio::test]
async fn admin_activity_returns_empty_without_history_provider() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/activity", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response)
            .await
            .as_array()
            .expect("activity")
            .len(),
        0
    );
}

#[tokio::test]
async fn admin_activity_limits_history_to_30_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(ActivityLookup));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/activity", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    let items = value.as_array().expect("activity array");
    assert_eq!(items.len(), 30);
    assert_eq!(items[0]["id"], "REC-000");
    assert_eq!(items[29]["id"], "REC-029");
}

#[tokio::test]
async fn admin_settings_put_updates_auth_runtime_like_go() {
    let mut state = test_state();
    state.admin = state
        .admin
        .clone()
        .with_auth_config_sink(Arc::new(state.auth.clone()));
    let token = session(&state, PrincipalRole::Admin).await;

    let update = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/settings",
            &token,
            r#"{
                "default_target_warehouse":"Stores - CH",
                "default_uom":"Kg",
                "werka_phone":"+998881111111",
                "werka_name":"Updated Werka",
                "werka_code":"20UPDATED",
                "werka_code_locked":false,
                "werka_code_retry_after_sec":0,
                "admin_phone":"+998882222222",
                "admin_name":"Updated Admin"
            }"#,
        ))
        .await
        .expect("response");
    assert_eq!(update.status(), StatusCode::OK);

    let old = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            r#"{"phone":"+998881111111","code":"20ABCDEF1234"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(old.status(), StatusCode::UNAUTHORIZED);

    let new = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            r#"{"phone":"+998881111111","code":"20UPDATED"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(new.status(), StatusCode::OK);
    let value = json_body(new).await;
    assert_eq!(value["profile"]["role"], "werka");
    assert_eq!(value["profile"]["display_name"], "Updated Werka");
}

#[tokio::test]
async fn admin_create_supplier_and_customer_mutations_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let supplier = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/suppliers",
            &token,
            r#"{"name":"New Supplier","phone":"+998909999999"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(supplier.status(), StatusCode::OK);
    let value = json_body(supplier).await;
    assert_eq!(value["ref"], "SUP-NEW");
    assert_eq!(value["phone"], "+998909999999");

    let customer = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/customers",
            &token,
            r#"{"name":"New Customer","phone":"+998901234567"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(customer.status(), StatusCode::OK);
    let value = json_body(customer).await;
    assert_eq!(value["ref"], "CUST-NEW");
    assert_eq!(value["name"], "New Customer");
}

#[tokio::test]
async fn admin_create_customer_rejects_duplicate_phone() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/customers",
            &token,
            r#"{"name":"Duplicate Customer","phone":"+998904444444"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "phone already exists");
}

#[tokio::test]
async fn admin_create_customer_rejects_local_format_duplicate_phone() {
    let mut state = test_state();
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(LocalPhoneDuplicateReadPort))
        .with_write_port(Arc::new(FakeAdminReadPort))
        .with_state_port(Arc::new(FakeAdminStatePort::new()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/customers",
            &token,
            r#"{"name":"Duplicate Customer","phone":"110000011"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "phone already exists");
}

#[tokio::test]
async fn admin_supplier_status_and_remove_mutations_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let status = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/suppliers/status?ref=SUP-001",
            &token,
            r#"{"blocked":true}"#,
        ))
        .await
        .expect("response");
    assert_eq!(status.status(), StatusCode::OK);
    assert_eq!(json_body(status).await["blocked"], true);

    let remove = build_router(state)
        .oneshot(request(
            "DELETE",
            "/v1/mobile/admin/suppliers/remove?ref=SUP-001",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(remove.status(), StatusCode::OK);
    assert_eq!(json_body(remove).await["ok"], true);
}

#[tokio::test]
async fn admin_item_mutation_errors_match_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let missing = build_router(state.clone())
        .oneshot(request(
            "DELETE",
            "/v1/mobile/admin/customers/items/remove?ref=CUST-001",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing).await["error"],
        "ref and item_code are required"
    );

    let invalid = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items/bulk-move-group",
            &token,
            r#"{"item_codes":[],"item_group":"Products"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(invalid).await["error"], "item codes are required");
}

#[tokio::test]
async fn admin_item_create_and_werka_regenerate_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let missing_customer = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items",
            &token,
            r#"{"code":"ITEM-FINISHED","name":"Finished Item","uom":"Kg","item_group":"tayyor mahsulot"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(missing_customer.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_customer).await["error"],
        "customer_ref is required for tayyor mahsulot"
    );

    let item = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items",
            &token,
            r#"{"code":"ITEM-NEW","name":"New Item","uom":"Kg","item_group":"Products"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(item.status(), StatusCode::OK);
    let value = json_body(item).await;
    assert_eq!(value["code"], "ITEM-NEW");
    assert_eq!(value["item_group"], "Products");

    let finished_item = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items",
            &token,
            r#"{"code":"ITEM-FINISHED","name":"Finished Item","uom":"Kg","item_group":"tayyor mahsulot","customer_ref":"CUST-001"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(finished_item.status(), StatusCode::OK);
    let value = json_body(finished_item).await;
    assert_eq!(value["code"], "ITEM-FINISHED");
    assert_eq!(value["item_group"], "tayyor mahsulot");

    let settings = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/werka/code/regenerate",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(settings.status(), StatusCode::OK);
    let value = json_body(settings).await;
    assert!(
        value["werka_code"]
            .as_str()
            .expect("code")
            .starts_with("20")
    );
}

#[tokio::test]
async fn material_taminotchi_item_create_is_limited_to_assigned_item_groups() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-create".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Kraska".to_string()],
        })
        .await
        .expect("material scope");
    let token = session_for(&state, PrincipalRole::MaterialTaminotchi, "material-create").await;
    let router = build_router(state);

    let blocked = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items",
            &token,
            r#"{"code":"GLUE-NEW","name":"Glue","uom":"Kg","item_group":"Kley"}"#,
        ))
        .await
        .expect("blocked response");
    assert_eq!(blocked.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(blocked).await["error"],
        "item group is not assigned to material taminotchi"
    );

    let allowed = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/items",
            &token,
            r#"{"code":"INK-NEW","name":"Ink","uom":"Kg","item_group":"Kraska"}"#,
        ))
        .await
        .expect("allowed response");
    let allowed_status = allowed.status();
    let allowed_body = json_body(allowed).await;
    assert_eq!(allowed_status, StatusCode::OK, "{allowed_body}");
    assert_eq!(allowed_body["item_group"], "Kraska");
}
