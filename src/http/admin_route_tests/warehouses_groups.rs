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
            r#"{"warehouse":" Ombor ","principal_role":"supplier","principal_ref":"SUP-001","display_name":"Supplier One"}"#,
        ))
        .await
        .expect("assign warehouse");
    assert_eq!(created.status(), StatusCode::OK);
    let created_body = json_body(created).await;
    assert_eq!(created_body["warehouse"], "Ombor");
    assert_eq!(created_body["principal_role"], "supplier");
    assert_eq!(created_body["principal_ref"], "SUP-001");
    assert_eq!(created_body["display_name"], "Supplier One");

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
    assert_eq!(listed_body[0]["principal_ref"], "SUP-001");
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
            r#"{"warehouse":" Ombor ","principal_role":"supplier","principal_ref":"SUP-001","display_name":"Supplier One"}"#,
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
    assert_eq!(body[0]["assigned_display_names"][0], "Supplier One");
}
