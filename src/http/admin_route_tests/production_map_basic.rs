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
