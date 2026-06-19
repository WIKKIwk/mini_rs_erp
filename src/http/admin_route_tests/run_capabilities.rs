use super::*;

#[tokio::test]
async fn admin_production_map_run_returns_calculated_tasks() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"hotlunch-test",
                "product_code":"HOTLUNCH",
                "title":"Hotlunch test",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {
                        "id":"formula",
                        "kind":"formula",
                        "title":"CPP hisob",
                        "formula":{"target":"cpp_kg","expression":"order_qty * 1.08"}
                    },
                    {
                        "id":"task",
                        "kind":"task",
                        "title":"Rezkaga yuborish",
                        "role_code":"rezkachi",
                        "qty_formula":"cpp_kg",
                        "from_location":"CPP ombor",
                        "to_location":"Rezka apparat"
                    },
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"formula"},
                    {"from":"formula","to":"task"},
                    {"from":"task","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/run",
            &token,
            r#"{"map_id":"hotlunch-test","order_qty":100}"#,
        ))
        .await
        .expect("run response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["variables"]["cpp_kg"], 108.0);
    assert_eq!(value["tasks"][0]["task_kind"], "create_task");
    assert_eq!(value["tasks"][0]["role_code"], "rezkachi");
    assert_eq!(value["tasks"][0]["qty"], 108.0);
    assert_eq!(value["tasks"][0]["from_location"], "CPP ombor");
    assert_eq!(value["tasks"][0]["to_location"], "Rezka apparat");
}

#[tokio::test]
async fn production_map_manage_capability_can_save_maps() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/roles",
            &admin_token,
            r#"{
                "id":"production_mapper",
                "label":"Production mapper",
                "capability_codes":["production.map.manage"]
            }"#,
        ))
        .await
        .expect("role response");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/roles", &admin_token))
        .await
        .expect("roles response");
    assert_eq!(response.status(), StatusCode::OK);
    let roles = json_body(response).await;
    assert!(
        roles
            .as_array()
            .expect("roles")
            .iter()
            .any(|role| role["id"] == "aparatchi")
    );

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &admin_token,
            r#"{
                "principal_role":"werka",
                "principal_ref":"werka",
                "role_id":"production_mapper"
            }"#,
        ))
        .await
        .expect("assignment response");
    assert_eq!(response.status(), StatusCode::OK);

    let mapper_token = session_for(&state, PrincipalRole::Werka, "werka").await;
    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &mapper_token,
            r#"{
                "id":"hotlunch-test",
                "product_code":"HOTLUNCH",
                "title":"Hotlunch test",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[{"from":"start","to":"end"}]
            }"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn apparatus_queue_read_capability_can_only_read_production_maps() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            r#"{
                "id":"queue-test",
                "product_code":"HOTLUNCH",
                "title":"Queue test",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"apparatus","kind":"apparatus","title":"Godex aparat - DEMO"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"apparatus"},
                    {"from":"apparatus","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save map");
    assert_eq!(response.status(), StatusCode::OK);

    let response = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/roles", &admin_token))
        .await
        .expect("roles response");
    assert_eq!(response.status(), StatusCode::OK);
    let roles = json_body(response).await;
    assert!(
        roles
            .as_array()
            .expect("roles")
            .iter()
            .any(|role| role["id"] == "aparatchi"),
        "{roles}"
    );

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/role-assignments",
            &admin_token,
            r#"{
                "principal_role":"werka",
                "principal_ref":"werka",
                "role_id":"aparatchi",
                "assigned_apparatus":["Godex aparat - DEMO"]
            }"#,
        ))
        .await
        .expect("assignment response");
    let status = response.status();
    let body = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["assigned_apparatus"],
        serde_json::json!(["Godex aparat - DEMO"])
    );

    let queue_token = session_for(&state, PrincipalRole::Werka, "werka").await;
    let response = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps",
            &queue_token,
        ))
        .await
        .expect("read response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await[0]["map"]["id"], "queue-test");

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &queue_token,
            r#"{
                "id":"queue-test-2",
                "product_code":"HOTLUNCH",
                "title":"Queue test 2",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[{"from":"start","to":"end"}]
            }"#,
        ))
        .await
        .expect("write response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_access_capability_can_save_production_maps() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/roles",
            &admin_token,
            r#"{
                "id":"admin_only",
                "label":"Admin only",
                "capability_codes":["admin.access"]
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
                "role_id":"admin_only"
            }"#,
        ))
        .await
        .expect("assignment response");
    assert_eq!(response.status(), StatusCode::OK);

    let admin_only_token = session_for(&state, PrincipalRole::Werka, "werka").await;
    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_only_token,
            r#"{
                "id":"hotlunch-test",
                "product_code":"HOTLUNCH",
                "title":"Hotlunch test",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[{"from":"start","to":"end"}]
            }"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}
