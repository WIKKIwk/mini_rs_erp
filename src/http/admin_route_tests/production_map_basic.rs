use super::*;

#[tokio::test]
async fn worker_conditional_snapshot_checks_revision_scope_and_authentication() {
    let state = test_state();
    let assignment = |apparatus: &str| crate::core::authz::RoleAssignmentUpsert {
        principal_role: PrincipalRole::Aparatchi,
        principal_ref: "conditional-worker".into(), role_id: "aparatchi".into(),
        assigned_apparatus: vec![apparatus.into()], assigned_item_groups: vec![],
    };
    state.admin.upsert_role_assignment(assignment("apparatus:default:bosma_7")).await.unwrap();
    let token = session_for(&state, PrincipalRole::Aparatchi, "conditional-worker").await;
    let router = build_router(state.clone());
    let base = "/v1/mobile/admin/production-maps/sequence?worker_scope=true";
    let response = router.clone().oneshot(request("GET", base, &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let first = json_body(response).await;
    assert!(first["completed_orders"].is_array());
    let uri = format!("{base}&if_epoch={}&if_rev={}&if_scope={}&wait_ms=10",
        first["epoch"].as_str().unwrap(), first["rev"], first["scope"].as_str().unwrap());
    let unchanged = router.clone().oneshot(request("GET", &uri, &token)).await.unwrap();
    assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(unchanged.headers()["cache-control"], "no-store");
    assert!(to_bytes(unchanged.into_body(), 100).await.unwrap().is_empty());

    // An event during the long poll must produce the new authoritative view.
    let wait_uri = uri.replace("wait_ms=10", "wait_ms=6000");
    let (changed, ()) = tokio::join!(
        router.clone().oneshot(request("GET", &wait_uri, &token)),
        async { tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            state.production_maps.notify_live(); },
    );
    let changed = changed.unwrap();
    assert_eq!(changed.status(), StatusCode::OK);
    let changed = json_body(changed).await;
    assert!(changed["rev"].as_u64().unwrap() > first["rev"].as_u64().unwrap());
    let current_uri = format!("{base}&if_epoch={}&if_rev={}&if_scope={}",
        changed["epoch"].as_str().unwrap(), changed["rev"], changed["scope"].as_str().unwrap());
    state.admin.upsert_role_assignment(assignment("apparatus:default:bosma_9")).await.unwrap();
    let moved = router.clone().oneshot(request("GET", &current_uri, &token)).await.unwrap();
    assert_eq!(moved.status(), StatusCode::OK);
    let moved = json_body(moved).await;
    assert_eq!(moved["rev"], changed["rev"]);
    assert_ne!(moved["scope"], changed["scope"]);
    let unauthorized = router.oneshot(request("GET", &current_uri, "invalid-token")).await.unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
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
                        "title":"apparatus:default:bosma_7",
                        "apparatus_id":"apparatus:default:bosma_7",
                        "alternative_group_id":"alt-pechat-1",
                        "alternative_group_label":"pechat",
                        "alternative_assigned_title":"apparatus:default:bosma_7",
                        "alternative_assigned_apparatus_id":"apparatus:default:bosma_7"
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
        "apparatus:default:bosma_7"
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
        "apparatus:default:bosma_7"
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
                        "apparatus_id":"apparatus:default:asset-010",
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
                    {"id":"pechat","kind":"apparatus","title":"apparatus:default:bosma_7","apparatus_id":"apparatus:default:bosma_7"},
                    {
                        "id":"lamin1",
                        "kind":"apparatus",
                        "title":"apparatus:default:asset-007",
                        "apparatus_id":"apparatus:default:asset-007",
                        "alternative_group_id":"alt-laminatsiya",
                        "alternative_group_label":"Laminatsiya",
                        "alternative_assigned_title":"apparatus:default:asset-007",
                        "alternative_assigned_apparatus_id":"apparatus:default:asset-007"
                    },
                    {
                        "id":"lamin2",
                        "kind":"apparatus",
                        "title":"apparatus:default:asset-008",
                        "apparatus_id":"apparatus:default:asset-008",
                        "alternative_group_id":"alt-laminatsiya",
                        "alternative_group_label":"Laminatsiya",
                        "alternative_assigned_title":"apparatus:default:asset-007",
                        "alternative_assigned_apparatus_id":"apparatus:default:asset-007"
                    },
                    {"id":"rezka","kind":"apparatus","title":"Rezka","apparatus_id":"apparatus:default:asset-010","rezka_kadr_count":4,"rezka_label_length":100},
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
        body["visible_order_ids"]["apparatus:default:bosma_7"],
        serde_json::json!(["zakaz-visible-alt"])
    );
    assert_eq!(
        body["visible_order_ids"]["apparatus:default:asset-007"],
        serde_json::json!(["zakaz-visible-alt"])
    );
    assert_eq!(
        body["visible_order_ids"]["apparatus:default:asset-010"],
        serde_json::json!(["zakaz-visible-alt"])
    );
    assert!(body["visible_order_ids"]["apparatus:default:asset-008"].is_null());
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
                    {"id":"pechat","kind":"apparatus","title":"apparatus:default:bosma_7","apparatus_id":"apparatus:default:bosma_7"},
                    {"id":"lamin","kind":"apparatus","title":"apparatus:default:asset-007","apparatus_id":"apparatus:default:asset-007"},
                    {"id":"rezka","kind":"apparatus","title":"Rezka","apparatus_id":"apparatus:default:asset-010","rezka_kadr_count":4,"rezka_label_length":100},
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
                    {"id":"pechat","kind":"apparatus","title":"apparatus:default:bosma_7","apparatus_id":"apparatus:default:bosma_7"},
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
        body["visible_order_ids"]["apparatus:default:bosma_7"],
        serde_json::json!(["1111"])
    );
    assert_eq!(
        body["visible_order_ids"]["apparatus:default:asset-007"],
        serde_json::json!(["1111"])
    );
    assert_eq!(
        body["visible_order_ids"]["apparatus:default:asset-010"],
        serde_json::json!(["1111"])
    );
}
