use super::*;

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
                    {"id":"apparatus","kind":"apparatus","title":"Godex aparat - DEMO","apparatus_id":"apparatus:default:bosma_7"},
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
                "assigned_apparatus":["apparatus:default:bosma_7"]
            }"#,
        ))
        .await
        .expect("assignment response");
    let status = response.status();
    let body = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["assigned_apparatus"],
        serde_json::json!(["apparatus:default:bosma_7"])
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
async fn material_taminotchi_can_read_production_maps_for_assignment_only() {
    let state = test_state();
    let admin_token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            r#"{
                "id":"material-assignment-test",
                "product_code":"HOTLUNCH",
                "title":"Material assignment test",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[{"from":"start","to":"end"}]
            }"#,
        ))
        .await
        .expect("save map");
    assert_eq!(response.status(), StatusCode::OK);

    let material_token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material_taminotchi",
    )
    .await;
    let response = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps",
            &material_token,
        ))
        .await
        .expect("read response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await[0]["map"]["id"],
        "material-assignment-test"
    );

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &material_token,
            r#"{
                "id":"material-assignment-test-2",
                "product_code":"HOTLUNCH",
                "title":"Material assignment test 2",
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
async fn qolipchi_and_material_taminotchi_can_read_order_sequence_but_not_reorder() {
    let state = test_state();
    let qolipchi_token = session_for(&state, PrincipalRole::Qolipchi, "qolipchi").await;
    let material_token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material_taminotchi",
    )
    .await;

    for (role, token) in [
        ("qolipchi", qolipchi_token),
        ("material_taminotchi", material_token),
    ] {
        for path in [
            "/v1/mobile/admin/production-maps",
            "/v1/mobile/admin/production-maps/sequence",
            "/v1/mobile/admin/apparatus?limit=200",
        ] {
            let response = build_router(state.clone())
                .oneshot(request("GET", path, &token))
                .await
                .expect("read sequence dependency");
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{role} must be able to read {path}"
            );
        }

        let response = build_router(state.clone())
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps/sequence",
                &token,
                r#"{"apparatus":"Bobst 1","order_ids":[]}"#,
            ))
            .await
            .expect("reorder response");
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{role} must not be able to reorder apparatus queues"
        );
    }
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
