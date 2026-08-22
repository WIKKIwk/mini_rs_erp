use super::*;

#[tokio::test]
async fn production_map_audit_route_returns_report() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/audit",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["ok"], true);
    assert_eq!(value["checked_order_count"], 0);
    assert_eq!(value["violations"].as_array().expect("violations").len(), 0);
}

#[tokio::test]
async fn admin_production_maps_save_compiles_program() {
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
                        "item_code":"CPP",
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
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["map"]["id"], "hotlunch-test");
    assert_eq!(value["program"]["operations"][1]["op_code"], "calculate");
    assert_eq!(
        value["program"]["operations"][1]["args"]["expression"],
        "order_qty * 1.08"
    );

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("response");
    assert_eq!(list.status(), StatusCode::OK);
    assert_eq!(json_body(list).await[0]["map"]["product_code"], "HOTLUNCH");
}

#[tokio::test]
async fn production_map_nodes_preserve_alternative_group_metadata() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-alt",
                "product_code":"ALT-001",
                "title":"Alternative order",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {
                        "id":"apparatus",
                        "kind":"apparatus",
                        "title":"7 ta rangli pechat",
                        "alternative_group_id":"alt-pechat-1",
                        "alternative_group_label":"pechat",
                        "alternative_assigned_title":"7 ta rangli pechat"
                    },
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"apparatus"},
                    {"from":"apparatus","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(
        value["map"]["nodes"][1]["alternative_group_id"],
        "alt-pechat-1"
    );
    assert_eq!(
        value["map"]["nodes"][1]["alternative_group_label"],
        "pechat"
    );
    assert_eq!(
        value["map"]["nodes"][1]["alternative_assigned_title"],
        "7 ta rangli pechat"
    );

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("response");
    assert_eq!(list.status(), StatusCode::OK);
    let listed = json_body(list).await;
    assert_eq!(
        listed[0]["map"]["nodes"][1]["alternative_group_id"],
        "alt-pechat-1"
    );
    assert_eq!(
        listed[0]["map"]["nodes"][1]["alternative_group_label"],
        "pechat"
    );
    assert_eq!(
        listed[0]["map"]["nodes"][1]["alternative_assigned_title"],
        "7 ta rangli pechat"
    );
}

#[tokio::test]
async fn production_map_nodes_preserve_rezka_setup_metadata() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-rezka-meta",
                "product_code":"REZKA-001",
                "title":"Rezka setup order",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {
                        "id":"rezka",
                        "kind":"apparatus",
                        "title":"Rezka",
                        "rezka_kadr_count":4,
                        "rezka_label_length":125.5
                    },
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"rezka"},
                    {"from":"rezka","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["map"]["nodes"][1]["rezka_kadr_count"], 4);
    assert_eq!(value["map"]["nodes"][1]["rezka_label_length"], 125.5);
    assert_eq!(
        value["program"]["operations"][1]["args"]["rezka_kadr_count"],
        "4"
    );
    assert_eq!(
        value["program"]["operations"][1]["args"]["rezka_label_length"],
        "125.5"
    );

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("response");
    assert_eq!(list.status(), StatusCode::OK);
    let listed = json_body(list).await;
    assert_eq!(listed[0]["map"]["nodes"][1]["rezka_kadr_count"], 4);
    assert_eq!(listed[0]["map"]["nodes"][1]["rezka_label_length"], 125.5);
}

#[tokio::test]
async fn production_map_sequence_returns_backend_visible_order_ids() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-visible-alt",
                "product_code":"ALT-PECH",
                "title":"Visible alternative order",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"order","kind":"task","title":"Visible product"},
                    {"id":"pechat","kind":"apparatus","title":"7 ta rangli pechat"},
                    {
                        "id":"lamin1",
                        "kind":"apparatus",
                        "title":"Laminatsiya 1",
                        "alternative_group_id":"alt-laminatsiya",
                        "alternative_group_label":"Laminatsiya",
                        "alternative_assigned_title":"Laminatsiya 1"
                    },
                    {
                        "id":"lamin2",
                        "kind":"apparatus",
                        "title":"Laminatsiya 2",
                        "alternative_group_id":"alt-laminatsiya",
                        "alternative_group_label":"Laminatsiya",
                        "alternative_assigned_title":"Laminatsiya 1"
                    },
                    {"id":"rezka","kind":"apparatus","title":"Rezka"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"order"},
                    {"from":"order","to":"pechat"},
                    {"from":"pechat","to":"lamin1"},
                    {"from":"lamin1","to":"rezka"},
                    {"from":"rezka","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save map");
    assert_eq!(save.status(), StatusCode::OK);

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &token,
        ))
        .await
        .expect("sequence");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;

    assert_eq!(
        body["visible_order_ids"]["7 ta rangli pechat"],
        serde_json::json!(["zakaz-visible-alt"])
    );
    assert_eq!(
        body["visible_order_ids"]["Laminatsiya 1"],
        serde_json::json!(["zakaz-visible-alt"])
    );
    assert_eq!(
        body["visible_order_ids"]["Rezka"],
        serde_json::json!(["zakaz-visible-alt"])
    );
    assert!(body["visible_order_ids"]["Laminatsiya 2"].is_null());
}

#[tokio::test]
async fn production_map_sequence_exposes_backend_worker_contract_revision() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-mobile-contract".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(
        &state,
        PrincipalRole::Aparatchi,
        "worker-mobile-contract",
    )
    .await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-mobile-contract",
                "Mobile contract order",
                "9901",
                "7 ta rangli pechat",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    let sequence = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_ids":["zakaz-mobile-contract"]
            }"#,
        ))
        .await
        .expect("save sequence");
    assert_eq!(sequence.status(), StatusCode::OK);

    let response = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker_token,
        ))
        .await
        .expect("sequence");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;

    assert_eq!(
        body["assigned_apparatus"],
        serde_json::json!(["7 ta rangli pechat"])
    );
    let revision = body["snapshot_revision"].as_str().expect("snapshot revision");
    assert_eq!(revision.len(), 64);
    assert!(revision.bytes().all(|byte| byte.is_ascii_hexdigit()));

    let interaction = &body["queue_action_controls"]["7 ta rangli pechat"]
        ["zakaz-mobile-contract"]["interaction"];
    for field in [
        "mode",
        "start_materials_mode",
        "previous_wip_mode",
        "qolip_mode",
        "blocking_reason_code",
    ] {
        assert!(interaction[field].is_string(), "missing interaction.{field}");
    }
    for field in [
        "material_scan_required",
        "assigned_materials_display_only",
        "material_intake_allowed",
    ] {
        assert!(interaction[field].is_boolean(), "missing interaction.{field}");
    }
    assert!(body["queue_action_controls"]["7 ta rangli pechat"]
        ["zakaz-mobile-contract"]["freeze_request"]
        .is_null());
}

#[tokio::test]
async fn production_map_queue_action_rejects_stale_snapshot_with_refresh_contract() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-mobile-stale".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-mobile-stale").await;
    let router = build_router(state);

    let saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-mobile-stale",
                "Mobile stale order",
                "9902",
                "7 ta rangli pechat",
            ),
        ))
        .await
        .expect("save map");
    assert_eq!(saved.status(), StatusCode::OK);

    let sequence = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/sequence",
            &admin_token,
            r#"{
                "apparatus":"7 ta rangli pechat",
                "order_ids":["zakaz-mobile-stale"]
            }"#,
        ))
        .await
        .expect("save sequence");
    assert_eq!(sequence.status(), StatusCode::OK);

    let snapshot = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker_token,
        ))
        .await
        .expect("snapshot");
    assert_eq!(snapshot.status(), StatusCode::OK);
    let snapshot_body = json_body(snapshot).await;
    let old_revision = snapshot_body["snapshot_revision"]
        .as_str()
        .expect("old snapshot revision")
        .to_string();

    let second_saved = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-mobile-stale-2",
                "Mobile stale order 2",
                "9903",
                "7 ta rangli pechat",
            ),
        ))
        .await
        .expect("save concurrent map");
    assert_eq!(second_saved.status(), StatusCode::OK);

    let action = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &format!(
                r#"{{
                    "apparatus":"7 ta rangli pechat",
                    "order_id":"zakaz-mobile-stale",
                    "action":"start",
                    "expected_snapshot_revision":"{old_revision}"
                }}"#
            ),
        ))
        .await
        .expect("stale action");
    assert_eq!(action.status(), StatusCode::CONFLICT);
    let action_body = json_body(action).await;
    assert_eq!(action_body["error"], "stale_production_snapshot");
    assert_eq!(action_body["refresh_required"], true);
    assert_eq!(
        action_body["refresh_endpoint"],
        "/v1/mobile/admin/production-maps/sequence"
    );
    let current_revision = action_body["snapshot_revision"]
        .as_str()
        .expect("current snapshot revision");
    assert_ne!(current_revision, old_revision);

    let refreshed = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &worker_token,
        ))
        .await
        .expect("refresh snapshot");
    assert_eq!(refreshed.status(), StatusCode::OK);
    assert_eq!(
        json_body(refreshed).await["snapshot_revision"],
        current_revision
    );
}

#[tokio::test]
async fn production_map_sequence_accepts_numeric_order_id() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"1111",
                "product_code":"FUNCHUZA",
                "title":"Funchuza 300 gr kok",
                "code":"1111",
                "order_number":"1111",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"order","kind":"task","title":"Funchuza 300 gr kok"},
                    {"id":"pechat","kind":"apparatus","title":"7 ta rangli pechat"},
                    {"id":"lamin","kind":"apparatus","title":"Laminatsiya 1"},
                    {"id":"rezka","kind":"apparatus","title":"Rezka"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"order"},
                    {"from":"order","to":"pechat"},
                    {"from":"pechat","to":"lamin"},
                    {"from":"lamin","to":"rezka"},
                    {"from":"rezka","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save map");
    assert_eq!(save.status(), StatusCode::OK);

    let template = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"template-1111",
                "product_code":"FUNCHUZA",
                "title":"Funchuza template",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"pechat","kind":"apparatus","title":"7 ta rangli pechat"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"pechat"},
                    {"from":"pechat","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save template map");
    assert_eq!(template.status(), StatusCode::OK);

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &token,
        ))
        .await
        .expect("sequence");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;

    assert_eq!(
        body["visible_order_ids"]["7 ta rangli pechat"],
        serde_json::json!(["1111"])
    );
    assert_eq!(
        body["visible_order_ids"]["Laminatsiya 1"],
        serde_json::json!(["1111"])
    );
    assert_eq!(
        body["visible_order_ids"]["Rezka"],
        serde_json::json!(["1111"])
    );
}
