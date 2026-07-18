use super::*;

#[tokio::test]
async fn admin_warehouses_returns_real_warehouse_names() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses?q=Stores&limit=5",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body[0]["warehouse"], "Stores - CH");
    assert_eq!(body[0]["company"], "Company");
}

#[tokio::test]
async fn admin_warehouses_filters_by_parent() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses?parent=Aparat&limit=5",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(body.as_array().expect("array").iter().any(|item| {
        item["warehouse"] == "Godex aparat - CH" && item["parent_warehouse"] == "aparat - A"
    }));
}

#[tokio::test]
async fn warehouse_items_are_filtered_searched_and_paginated_on_the_backend() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let first = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses/items?warehouse=Stores%20-%20CH&limit=1",
            &token,
        ))
        .await
        .expect("first warehouse item page");
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = json_body(first).await;
    assert_eq!(first_body.as_array().expect("array").len(), 1);
    assert_eq!(first_body[0]["code"], "ITEM-001");

    let second = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses/items?warehouse=Stores%20-%20CH&limit=1&offset=1",
            &token,
        ))
        .await
        .expect("second warehouse item page");
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(json_body(second).await[0]["code"], "INK-BLACK");

    let searched = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses/items?warehouse=Stores%20-%20CH&q=black&limit=80",
            &token,
        ))
        .await
        .expect("searched warehouse items");
    assert_eq!(searched.status(), StatusCode::OK);
    let searched_body = json_body(searched).await;
    assert_eq!(searched_body.as_array().expect("array").len(), 1);
    assert_eq!(searched_body[0]["code"], "INK-BLACK");
}

#[tokio::test]
async fn admin_apparatus_defaults_are_available_on_empty_store() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let groups = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/apparatus-groups", &token))
        .await
        .expect("groups response");
    assert_eq!(groups.status(), StatusCode::OK);
    let group_body = json_body(groups).await;
    assert_eq!(group_body[0]["name"], "Bosma aparat");
    assert_eq!(
        group_body[0]["apparatus"],
        serde_json::json!([
            "7 ta rangli bosma aparat",
            "8 ta rangli bosma aparat",
            "9 ta rangli bosma aparat"
        ])
    );
    assert_eq!(group_body[1]["name"], "Laminatsiya");
    assert_eq!(
        group_body[1]["apparatus"],
        serde_json::json!(["Laminatsiya 1", "Laminatsiya 2"])
    );
    assert_eq!(group_body[2]["name"], "Rezka");
    assert_eq!(group_body[2]["apparatus"], serde_json::json!(["Rezka"]));

    let apparatus = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses?parent=aparat%20-%20A&limit=50",
            &token,
        ))
        .await
        .expect("apparatus response");
    assert_eq!(apparatus.status(), StatusCode::OK);
    let apparatus_body = json_body(apparatus).await;
    for expected in [
        "7 ta rangli bosma aparat",
        "8 ta rangli bosma aparat",
        "9 ta rangli bosma aparat",
        "Laminatsiya 1",
        "Laminatsiya 2",
        "Rezka",
    ] {
        assert!(
            apparatus_body
                .as_array()
                .expect("array")
                .iter()
                .any(|item| item["warehouse"] == expected),
            "missing default apparatus {expected}"
        );
    }
}

#[tokio::test]
async fn admin_apparatus_groups_round_trip_on_server() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let saved = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/apparatus-groups",
            &token,
            r#"{"name":" pechat ","apparatus":[" 7 ta rangli pechat ","8 ta rangli pechat","7 ta rangli pechat"]}"#,
        ))
        .await
        .expect("response");
    assert_eq!(saved.status(), StatusCode::OK);
    let saved_body = json_body(saved).await;
    assert_eq!(saved_body["name"], "Bosma aparat");
    assert_eq!(
        saved_body["apparatus"],
        serde_json::json!([
            "7 ta rangli bosma aparat",
            "8 ta rangli bosma aparat",
            "9 ta rangli bosma aparat"
        ])
    );

    let listed = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/apparatus-groups", &token))
        .await
        .expect("response");
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = json_body(listed).await;
    assert_eq!(listed_body[0]["name"], "Bosma aparat");
    assert_eq!(
        listed_body[0]["apparatus"],
        serde_json::json!([
            "7 ta rangli bosma aparat",
            "8 ta rangli bosma aparat",
            "9 ta rangli bosma aparat"
        ])
    );
}

#[tokio::test]
async fn admin_apparatus_group_keeps_laminatsiya_apparatus_manual() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let saved = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/apparatus-groups",
            &token,
            r#"{"name":" laminatsiya apparatlar ","apparatus":[" Laminatsiya 1 ","Laminatsiya kley"]}"#,
        ))
        .await
        .expect("response");
    assert_eq!(saved.status(), StatusCode::OK);
    let saved_body = json_body(saved).await;
    assert_eq!(saved_body["name"], "Laminatsiya");
    assert_eq!(
        saved_body["apparatus"],
        serde_json::json!(["Laminatsiya 1", "Laminatsiya kley"])
    );

    let listed = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/apparatus-groups", &token))
        .await
        .expect("response");
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = json_body(listed).await;
    assert_eq!(listed_body[0]["name"], "Laminatsiya");
    assert_eq!(
        listed_body[0]["apparatus"],
        serde_json::json!(["Laminatsiya 1", "Laminatsiya kley"])
    );
}

#[tokio::test]
async fn admin_can_create_apparatus_and_list_it_as_apparat_warehouse() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            r#"{"warehouse":" Bobst 1 "}"#,
        ))
        .await
        .expect("create apparatus");
    assert_eq!(created.status(), StatusCode::OK);
    let created_body = json_body(created).await;
    assert_eq!(created_body["warehouse"], "Bobst 1");
    assert_eq!(created_body["parent_warehouse"], "aparat - A");

    let listed = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses?parent=aparat%20-%20A&limit=50",
            &token,
        ))
        .await
        .expect("list apparatus");
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = json_body(listed).await;
    assert!(
        listed_body
            .as_array()
            .expect("array")
            .iter()
            .any(|item| item["warehouse"] == "Bobst 1")
    );
}

#[tokio::test]
async fn admin_can_create_general_warehouse_and_list_it_for_gscale() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses",
            &token,
            r#"{"warehouse":" Kalidor "}"#,
        ))
        .await
        .expect("create warehouse");
    assert_eq!(created.status(), StatusCode::OK);
    let created_body = json_body(created).await;
    assert_eq!(created_body["warehouse"], "Kalidor");
    assert_eq!(created_body["parent_warehouse"], "");

    let listed = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses?q=kal&limit=50",
            &token,
        ))
        .await
        .expect("list warehouse");
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = json_body(listed).await;
    assert!(
        listed_body
            .as_array()
            .expect("array")
            .iter()
            .any(|item| item["warehouse"] == "Kalidor")
    );

    let apparatus = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses?parent=aparat%20-%20A&limit=50",
            &token,
        ))
        .await
        .expect("list apparatus");
    assert_eq!(apparatus.status(), StatusCode::OK);
    let apparatus_body = json_body(apparatus).await;
    assert!(
        !apparatus_body
            .as_array()
            .expect("array")
            .iter()
            .any(|item| item["warehouse"] == "Kalidor")
    );
}

#[tokio::test]
async fn admin_can_assign_warehouse_to_user_and_list_assignments() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses/assignments",
            &token,
            r#"{"warehouse":" Ombor ","principal_role":"werka","principal_ref":"werka","display_name":"Werka"}"#,
        ))
        .await
        .expect("assign warehouse");
    assert_eq!(created.status(), StatusCode::OK);
    let created_body = json_body(created).await;
    assert_eq!(created_body["warehouse"], "Ombor");
    assert_eq!(created_body["principal_role"], "werka");
    assert_eq!(created_body["principal_ref"], "werka");
    assert_eq!(created_body["display_name"], "Werka");

    let listed = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses/assignments?warehouse=ombor",
            &token,
        ))
        .await
        .expect("list assignments");
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = json_body(listed).await;
    assert_eq!(listed_body[0]["warehouse"], "Ombor");
    assert_eq!(listed_body[0]["principal_ref"], "werka");
}

#[tokio::test]
async fn warehouse_assignment_accepts_only_allowed_roles_and_brigader_workers() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let brigader = state
        .workers
        .upsert_worker(WorkerUpsert {
            id: "brigader-1".to_string(),
            name: "Brigader One".to_string(),
            phone: String::new(),
            level: "Brigader".to_string(),
        })
        .await
        .expect("brigader");
    let master = state
        .workers
        .upsert_worker(WorkerUpsert {
            id: "master-1".to_string(),
            name: "Master One".to_string(),
            phone: String::new(),
            level: "Master".to_string(),
        })
        .await
        .expect("master");

    let allowed = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses/assignments",
            &token,
            &format!(
                r#"{{"warehouse":"Ombor","principal_role":"aparatchi","principal_ref":"{}","display_name":"Brigader One"}}"#,
                brigader.id
            ),
        ))
        .await
        .expect("assign brigader");
    assert_eq!(allowed.status(), StatusCode::OK);

    let master_rejected = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses/assignments",
            &token,
            &format!(
                r#"{{"warehouse":"Ombor","principal_role":"aparatchi","principal_ref":"{}","display_name":"Master One"}}"#,
                master.id
            ),
        ))
        .await
        .expect("reject master");
    assert_eq!(master_rejected.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(master_rejected).await["error"],
        "warehouse_assignee_not_allowed"
    );

    let supplier_rejected = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses/assignments",
            &token,
            r#"{"warehouse":"Ombor","principal_role":"supplier","principal_ref":"SUP-001","display_name":"Supplier One"}"#,
        ))
        .await
        .expect("reject supplier");
    assert_eq!(supplier_rejected.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(supplier_rejected).await["error"],
        "warehouse_assignee_not_allowed"
    );
}

#[tokio::test]
async fn admin_can_remove_one_warehouse_assignment() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let assignment =
        r#"{"warehouse":"Ombor","principal_role":"werka","principal_ref":"werka","display_name":"Werka"}"#;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses/assignments",
            &token,
            assignment,
        ))
        .await
        .expect("assign warehouse");
    assert_eq!(created.status(), StatusCode::OK);

    let removed = build_router(state.clone())
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/warehouses/assignments",
            &token,
            assignment,
        ))
        .await
        .expect("remove warehouse assignment");
    assert_eq!(removed.status(), StatusCode::OK);
    let removed_body = json_body(removed).await;
    assert_eq!(removed_body["ok"], true);
    assert_eq!(removed_body["assignment"]["warehouse"], "Ombor");
    assert_eq!(removed_body["assignment"]["principal_ref"], "werka");

    let listed = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses/assignments?warehouse=Ombor",
            &token,
        ))
        .await
        .expect("list assignments after remove");
    assert_eq!(listed.status(), StatusCode::OK);
    assert!(json_body(listed).await.as_array().unwrap().is_empty());

    let missing = build_router(state)
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/warehouses/assignments",
            &token,
            assignment,
        ))
        .await
        .expect("remove missing warehouse assignment");
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        json_body(missing).await["error"],
        "warehouse_assignment_not_found"
    );
}

#[tokio::test]
async fn admin_deletes_empty_warehouse_and_its_assignments() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses",
            &token,
            r#"{"warehouse":"Delete me"}"#,
        ))
        .await
        .expect("create warehouse");
    build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses/assignments",
            &token,
            r#"{"warehouse":"Delete me","principal_role":"werka","principal_ref":"werka","display_name":"Werka"}"#,
        ))
        .await
        .expect("assign warehouse");

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/warehouses",
            &token,
            r#"{"warehouse":"Delete me","delete_products":false}"#,
        ))
        .await
        .expect("delete warehouse");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["warehouse"], "Delete me");
    assert_eq!(body["deleted_product_count"], 0);
    assert_eq!(body["deleted_assignment_count"], 1);

    let assignments = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses/assignments?warehouse=Delete%20me",
            &token,
        ))
        .await
        .expect("list assignments");
    assert!(json_body(assignments).await.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn warehouse_products_require_confirmation_and_active_reservations_block_delete() {
    let mut state = test_state();
    let store = Arc::new(MemoryWarehouseStore::new());
    state.warehouses = WarehouseService::new(store.clone());
    let token = session(&state, PrincipalRole::Admin).await;

    build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses",
            &token,
            r#"{"warehouse":"Stock warehouse"}"#,
        ))
        .await
        .expect("create warehouse");
    store.set_summary_counts("Stock warehouse", 4, 1).await;

    let reserved = build_router(state.clone())
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/warehouses",
            &token,
            r#"{"warehouse":"Stock warehouse","delete_products":true}"#,
        ))
        .await
        .expect("reserved delete");
    assert_eq!(reserved.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(reserved).await["error"],
        "warehouse_has_active_reservations"
    );

    store.set_summary_counts("Stock warehouse", 4, 0).await;
    let unconfirmed = build_router(state.clone())
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/warehouses",
            &token,
            r#"{"warehouse":"Stock warehouse","delete_products":false}"#,
        ))
        .await
        .expect("unconfirmed delete");
    assert_eq!(unconfirmed.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(unconfirmed).await["error"], "warehouse_not_empty");

    let confirmed = build_router(state)
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/warehouses",
            &token,
            r#"{"warehouse":"Stock warehouse","delete_products":true}"#,
        ))
        .await
        .expect("confirmed delete");
    assert_eq!(confirmed.status(), StatusCode::OK);
    let body = json_body(confirmed).await;
    assert_eq!(body["deleted_product_count"], 4);
}

#[tokio::test]
async fn admin_warehouse_summary_returns_lightweight_counts() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses",
            &token,
            r#"{"warehouse":" Ombor "}"#,
        ))
        .await
        .expect("create warehouse");
    assert_eq!(created.status(), StatusCode::OK);

    let assigned = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses/assignments",
            &token,
            r#"{"warehouse":" Ombor ","principal_role":"werka","principal_ref":"werka","display_name":"Werka"}"#,
        ))
        .await
        .expect("assign warehouse");
    assert_eq!(assigned.status(), StatusCode::OK);

    let listed = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses/summary?q=ombor&limit=50",
            &token,
        ))
        .await
        .expect("summary");
    assert_eq!(listed.status(), StatusCode::OK);
    let body = json_body(listed).await;
    assert_eq!(body[0]["warehouse"], "Ombor");
    assert_eq!(body[0]["product_count"], 0);
    assert_eq!(body[0]["reserved_count"], 0);
    assert_eq!(body[0]["assignment_count"], 1);
    assert_eq!(body[0]["assigned_display_names"][0], "Werka");
}

#[tokio::test]
async fn material_taminotchi_warehouses_are_limited_to_assigned_warehouses() {
    let state = test_state();
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-warehouse-scope",
        "Kalidor",
    )
    .await;
    state
        .warehouses
        .upsert_warehouse(WarehouseUpsert {
            warehouse: "Boshqa ombor".to_string(),
            company: "Company".to_string(),
            is_group: false,
            parent_warehouse: String::new(),
        })
        .await
        .expect("other warehouse");
    let token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-warehouse-scope",
    )
    .await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses?limit=50",
            &token,
        ))
        .await
        .expect("warehouses response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let warehouses = body.as_array().expect("warehouse array");
    assert_eq!(warehouses.len(), 1, "{body}");
    assert_eq!(warehouses[0]["warehouse"], "Kalidor");
}

#[tokio::test]
async fn material_taminotchi_warehouse_summary_uses_assigned_warehouses_only() {
    let state = test_state();
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-summary-scope",
        "Kalidor",
    )
    .await;
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::Supplier,
        "SUP-001",
        "Boshqa ombor",
    )
    .await;
    let token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-summary-scope",
    )
    .await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses/summary?limit=50",
            &token,
        ))
        .await
        .expect("summary response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let summaries = body.as_array().expect("summary array");
    assert_eq!(summaries.len(), 1, "{body}");
    assert_eq!(summaries[0]["warehouse"], "Kalidor");
    assert_eq!(summaries[0]["assignment_count"], 1);
}

#[tokio::test]
async fn material_taminotchi_sees_only_own_warehouse_assignments() {
    let state = test_state();
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-own-warehouse",
        "Kalidor",
    )
    .await;
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "other-material",
        "Boshqa ombor",
    )
    .await;
    let token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-own-warehouse",
    )
    .await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses/assignments",
            &token,
        ))
        .await
        .expect("assignments response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let assignments = body.as_array().expect("assignments array");
    assert_eq!(assignments.len(), 1, "{body}");
    assert_eq!(assignments[0]["warehouse"], "Kalidor");
    assert_eq!(assignments[0]["principal_ref"], "material-own-warehouse");
}
