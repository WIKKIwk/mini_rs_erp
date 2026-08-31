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
    let mut state = test_state();
    let store = Arc::new(MemoryWarehouseStore::new());
    store
        .set_stock_items(vec![
            WarehouseStockItem {
                code: "FG-001".to_string(),
                name: "Finished One".to_string(),
                uom: "Dona".to_string(),
                warehouse: "Stores - CH".to_string(),
                item_group: "Finished Goods".to_string(),
                on_hand_qty: 12.0,
                package_count: 2,
            },
            WarehouseStockItem {
                code: "FG-BLACK".to_string(),
                name: "Black Finished Product".to_string(),
                uom: "Kg".to_string(),
                warehouse: "Stores - CH".to_string(),
                item_group: "Finished Goods".to_string(),
                on_hand_qty: 5.5,
                package_count: 1,
            },
        ])
        .await;
    state.warehouses = WarehouseService::new_for_test(store);
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
    assert_eq!(first_body[0]["code"], "FG-BLACK");
    assert_eq!(first_body[0]["on_hand_qty"], 5.5);
    assert_eq!(first_body[0]["package_count"], 1);

    let second = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses/items?warehouse=Stores%20-%20CH&limit=1&offset=1",
            &token,
        ))
        .await
        .expect("second warehouse item page");
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(json_body(second).await[0]["code"], "FG-001");

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
    assert_eq!(searched_body[0]["code"], "FG-BLACK");
}

#[tokio::test]
async fn legacy_apparatus_groups_and_warehouse_catalog_are_not_runtime_authorities() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let groups = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/apparatus-groups", &token))
        .await
        .expect("groups response");
    assert_eq!(groups.status(), StatusCode::NOT_FOUND);

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
    assert!(
        apparatus_body
            .as_array()
            .expect("array")
            .iter()
            .all(|item| item["id"].is_null()),
        "warehouse lookup must not inject an apparatus catalog"
    );
}

#[tokio::test]
async fn legacy_apparatus_upsert_payload_is_rejected() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let initial = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/apparatus?limit=50",
            &token,
        ))
        .await
        .expect("initial apparatus list");
    let initial_count = json_body(initial)
        .await
        .as_array()
        .expect("apparatus array")
        .len();

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            r#"{"name":"apparatus:default:bosma_7"}"#,
        ))
        .await
        .expect("create legacy apparatus name");
    assert_eq!(created.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(created).await["error"], "invalid json");

    let listed = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/apparatus?limit=50",
            &token,
        ))
        .await
        .expect("apparatus list");
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = json_body(listed).await;
    assert_eq!(
        listed_body.as_array().expect("apparatus array").len(),
        initial_count
    );
}

#[tokio::test]
async fn admin_apparatus_options_and_metadata_validation_are_enforced() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let options = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/apparatus/options", &token))
        .await
        .expect("apparatus options");
    assert_eq!(options.status(), StatusCode::OK);
    let options_body = json_body(options).await;
    assert_eq!(options_body["contract"], "canonical_apparatus_revision");
    assert!(
        options_body["vocabulary"]["execution_operations"]
            .as_array()
            .expect("execution operations")
            .iter()
            .any(|item| item == "print")
    );

    let invalid_family = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            r#"{"name":"Invalid family","family":"dshjkhgdsjhjksdh","kind":"other","capabilities":["apparatus"]}"#,
        ))
        .await
        .expect("invalid family");
    assert_eq!(invalid_family.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(invalid_family).await["error"], "invalid json");

    let invalid_kind = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            r#"{"name":"Invalid kind","family":"other","kind":"hgjhjkd","capabilities":["apparatus"]}"#,
        ))
        .await
        .expect("invalid kind");
    assert_eq!(invalid_kind.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(invalid_kind).await["error"], "invalid json");

    let invalid_capability = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            r#"{"name":"Invalid capability","family":"other","kind":"other","capabilities":["hgjhjkd"]}"#,
        ))
        .await
        .expect("invalid capability");
    assert_eq!(invalid_capability.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(invalid_capability).await["error"], "invalid json");

    let invalid_color_stations = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            r#"{"name":"Invalid color stations","family":"pechat","kind":"color_pechat","capabilities":["print","pechat"],"color_stations":25}"#,
        ))
        .await
        .expect("invalid color stations");
    assert_eq!(invalid_color_stations.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid_color_stations).await["error"],
        "invalid json"
    );
}

#[tokio::test]
async fn admin_can_create_list_and_rename_canonical_apparatus() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            &canonical_apparatus_draft_body("bobst-1", "Bobst 1"),
        ))
        .await
        .expect("create apparatus");
    assert_eq!(created.status(), StatusCode::OK);
    let created_body = json_body(created).await;
    assert_eq!(
        created_body["revision"]["display"]["display_name"],
        "Bobst 1"
    );
    let apparatus_id = created_body["revision"]["apparatus_id"]
        .as_str()
        .expect("generated apparatus id")
        .to_string();

    let typed_list = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/apparatus?q=Bobst&limit=50",
            &token,
        ))
        .await
        .expect("typed apparatus list");
    assert_eq!(typed_list.status(), StatusCode::OK);
    let typed_body = json_body(typed_list).await;
    assert_eq!(typed_body[0]["display"]["display_name"], "Bobst 1");
    assert_eq!(typed_body[0]["apparatus_id"], apparatus_id);

    let renamed = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            &format!("/v1/mobile/admin/apparatus/{apparatus_id}"),
            &token,
            &canonical_apparatus_update_body("bobst-1", "Bobst 2", 1),
        ))
        .await
        .expect("rename semantic apparatus");
    assert_eq!(renamed.status(), StatusCode::OK);
    let renamed_body = json_body(renamed).await;
    assert_eq!(renamed_body["revision"]["apparatus_id"], apparatus_id);
    assert_eq!(
        renamed_body["revision"]["display"]["display_name"],
        "Bobst 2"
    );

    let renamed_list = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/apparatus?q=Bobst%202&limit=50",
            &token,
        ))
        .await
        .expect("list renamed apparatus");
    let renamed_list_body = json_body(renamed_list).await;
    assert_eq!(renamed_list_body[0]["apparatus_id"], apparatus_id);
    assert_eq!(renamed_list_body[0]["display"]["display_name"], "Bobst 2");

    let default_update = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            r#"{"id":"apparatus:default:asset-005","name":"Flexo pechat"}"#,
        ))
        .await
        .expect("reject default apparatus update");
    assert_eq!(default_update.status(), StatusCode::BAD_REQUEST);

    let legacy_created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            r#"{"warehouse":" Legacy Bobst "}"#,
        ))
        .await
        .expect("legacy create apparatus");
    assert_eq!(legacy_created.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn canonical_apparatus_does_not_duplicate_the_true_warehouse_catalog() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let created_apparatus = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            &canonical_apparatus_draft_body("shared-name", "Shared name"),
        ))
        .await
        .expect("create apparatus");
    assert_eq!(created_apparatus.status(), StatusCode::OK);
    let created_apparatus_id = json_body(created_apparatus).await["revision"]["apparatus_id"]
        .as_str()
        .expect("generated apparatus id")
        .to_string();

    let created_warehouse = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses",
            &token,
            r#"{"warehouse":"Shared name","parent_warehouse":"aparat - A"}"#,
        ))
        .await
        .expect("create same-name warehouse");
    assert_eq!(created_warehouse.status(), StatusCode::OK);

    let listed = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses?parent=aparat%20-%20A&limit=50",
            &token,
        ))
        .await
        .expect("list apparatus compatibility entries");
    assert_eq!(listed.status(), StatusCode::OK);
    let matching = json_body(listed)
        .await
        .as_array()
        .expect("warehouse array")
        .iter()
        .filter(|entry| entry["warehouse"] == "Shared name")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1);
    assert!(matching[0].get("id").is_none());

    let canonical = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/apparatus?q=Shared%20name&limit=50",
            &token,
        ))
        .await
        .expect("canonical apparatus list");
    assert_eq!(
        json_body(canonical).await[0]["apparatus_id"],
        created_apparatus_id
    );
}

#[tokio::test]
async fn apparatus_compatibility_scope_survives_display_name_rename() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &admin_token,
            &canonical_apparatus_draft_body("rename-scope", "Before rename"),
        ))
        .await
        .expect("create apparatus");
    assert_eq!(created.status(), StatusCode::OK);
    let apparatus_id = json_body(created).await["revision"]["apparatus_id"]
        .as_str()
        .expect("generated apparatus id")
        .to_string();

    state
        .warehouses
        .assign_warehouse(WarehouseAssignmentUpsert {
            assignment_kind: "apparatus".to_string(),
            warehouse: apparatus_id.clone(),
            warehouse_name: None,
            apparatus_id: Some(apparatus_id.clone()),
            principal_role: PrincipalRole::Werka,
            principal_ref: "werka-apparatus-scope".to_string(),
            display_name: "Werka".to_string(),
        })
        .await
        .expect("assign canonical apparatus");

    let renamed = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            &format!("/v1/mobile/admin/apparatus/{apparatus_id}"),
            &admin_token,
            &canonical_apparatus_update_body("rename-scope", "After rename", 1),
        ))
        .await
        .expect("rename apparatus");
    assert_eq!(renamed.status(), StatusCode::OK);

    let listed = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/apparatus?q=After%20rename&limit=50",
            &admin_token,
        ))
        .await
        .expect("list canonical apparatus");
    assert_eq!(listed.status(), StatusCode::OK);
    let body = json_body(listed).await;
    let entries = body.as_array().expect("apparatus array");
    assert!(entries.iter().any(|entry| {
        entry["apparatus_id"] == apparatus_id && entry["display"]["display_name"] == "After rename"
    }));
    assert!(
        !entries
            .iter()
            .any(|entry| entry["display"]["display_name"] == "Before rename")
    );

    let assignments = state
        .warehouses
        .assigned_warehouse_keys(&Principal {
            role: PrincipalRole::Werka,
            display_name: "Werka".to_string(),
            legal_name: "Werka".to_string(),
            ref_: "werka-apparatus-scope".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .expect("assigned warehouse keys");
    assert_eq!(assignments, vec![apparatus_id]);
}

#[tokio::test]
async fn legacy_true_warehouse_scope_remains_name_keyed() {
    let state = test_state();
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::Werka,
        "werka-legacy-warehouse",
        "Legacy warehouse",
    )
    .await;
    let token = session_for(&state, PrincipalRole::Werka, "werka-legacy-warehouse").await;

    let listed = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses?limit=50",
            &token,
        ))
        .await
        .expect("list legacy warehouse");
    assert_eq!(listed.status(), StatusCode::OK);
    let body = json_body(listed).await;
    let entries = body.as_array().expect("warehouse array");
    assert_eq!(entries.len(), 1, "legacy scope should remain name-keyed");
    assert_eq!(entries[0]["warehouse"], "Legacy warehouse");
    assert!(entries[0].get("id").is_none());
}

#[tokio::test]
async fn apparatus_queue_reader_can_list_but_cannot_create_apparatus() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "apparatus-reader".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("apparatus reader assignment");
    let token = session_for(&state, PrincipalRole::Aparatchi, "apparatus-reader").await;

    let listed = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/apparatus?limit=20",
            &token,
        ))
        .await
        .expect("apparatus list");
    assert_eq!(listed.status(), StatusCode::OK);
    assert!(
        !json_body(listed)
            .await
            .as_array()
            .expect("apparatus array")
            .is_empty()
    );

    let create_attempt = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/apparatus",
            &token,
            r#"{"name":"Forbidden apparatus"}"#,
        ))
        .await
        .expect("create attempt");
    assert_eq!(create_attempt.status(), StatusCode::FORBIDDEN);
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
async fn apparatus_assignment_rejects_syntactically_valid_nonexistent_id() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses/assignments",
            &token,
            r#"{
                "assignment_kind":"apparatus",
                "warehouse":"apparatus:custom:does-not-exist",
                "apparatus_id":"apparatus:custom:does-not-exist",
                "principal_role":"werka",
                "principal_ref":"werka-missing-apparatus",
                "display_name":"Werka"
            }"#,
        ))
        .await
        .expect("reject missing apparatus assignment");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "apparatus is invalid");

    let assignments = state
        .warehouses
        .warehouse_assignments("")
        .await
        .expect("load assignments");
    assert!(assignments.is_empty());
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
    let assignment = r#"{"warehouse":"Ombor","principal_role":"werka","principal_ref":"werka","display_name":"Werka"}"#;

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
    state.warehouses = WarehouseService::new_for_test(store.clone());
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
async fn werka_gscale_warehouses_are_limited_to_assigned_warehouses() {
    let state = test_state();
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::Werka,
        "werka-warehouse-scope",
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
    let token = session_for(&state, PrincipalRole::Werka, "werka-warehouse-scope").await;

    let response = build_router(state.clone())
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

    let write_attempt = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses",
            &token,
            r#"{"warehouse":"Werka yaratgan ombor"}"#,
        ))
        .await
        .expect("warehouse write response");
    assert_eq!(write_attempt.status(), StatusCode::FORBIDDEN);

    let delete_attempt = build_router(state)
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/warehouses",
            &token,
            r#"{"warehouse":"Kalidor"}"#,
        ))
        .await
        .expect("warehouse delete response");
    assert_eq!(delete_attempt.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn werka_without_assignment_gets_an_empty_warehouse_catalog() {
    let state = test_state();
    let token = session_for(&state, PrincipalRole::Werka, "werka-without-warehouse").await;

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
    assert!(
        body.as_array().expect("warehouse array").is_empty(),
        "{body}"
    );
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
    assign_warehouse_to_principal(&state, PrincipalRole::Supplier, "SUP-001", "Boshqa ombor").await;
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
