use super::*;

#[tokio::test]
async fn production_map_batch_move_keeps_laminatsiya_alternatives_in_group() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-lamin-alt-move",
                "product_code":"LAMIN-ALT",
                "title":"Laminatsiya alternative move",
                "roll_count":7,
                "width_mm":900,
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {
                        "id":"lamin-1",
                        "kind":"apparatus",
                        "title":"Laminatsiya 1 - A",
                        "alternative_group_id":"alt-lamin",
                        "alternative_group_label":"laminatsiya",
                        "alternative_assigned_title":"Laminatsiya 1 - A"
                    },
                    {
                        "id":"lamin-2",
                        "kind":"apparatus",
                        "title":"Laminatsiya 2 - A",
                        "alternative_group_id":"alt-lamin",
                        "alternative_group_label":"laminatsiya",
                        "alternative_assigned_title":"Laminatsiya 1 - A"
                    },
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"lamin-1"},
                    {"from":"lamin-1","to":"end"},
                    {"from":"start","to":"lamin-2"},
                    {"from":"lamin-2","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save");
    assert_eq!(save.status(), StatusCode::OK);

    let moved = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"Laminatsiya 1 - A",
                "to_apparatus":"Laminatsiya 2 - A",
                "map_ids":["zakaz-lamin-alt-move"]
            }"#,
        ))
        .await
        .expect("move to laminatsiya");
    assert_eq!(moved.status(), StatusCode::OK);
    let moved_body = json_body(moved).await;
    let assigned_titles: Vec<&str> = moved_body["saved"][0]["map"]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .filter_map(|node| {
            (node["kind"] == "apparatus").then(|| node["alternative_assigned_title"].as_str())
        })
        .flatten()
        .collect();
    assert_eq!(
        assigned_titles,
        vec!["Laminatsiya 2 - A", "Laminatsiya 2 - A"]
    );

    let blocked = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"Laminatsiya 2 - A",
                "to_apparatus":"Paket aparat - A",
                "map_ids":["zakaz-lamin-alt-move"]
            }"#,
        ))
        .await
        .expect("move to paket");
    assert_eq!(blocked.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(blocked).await["error"], "move_not_allowed");

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    let assigned_after_block: Vec<&str> = maps[0]["map"]["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .filter_map(|node| {
            (node["kind"] == "apparatus").then(|| node["alternative_assigned_title"].as_str())
        })
        .flatten()
        .collect();
    assert_eq!(
        assigned_after_block,
        vec!["Laminatsiya 2 - A", "Laminatsiya 2 - A"]
    );
}

#[tokio::test]
async fn production_map_batch_move_is_all_or_nothing() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    for number in ["1010", "2020"] {
        let save = build_router(state.clone())
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &token,
                &pechat_order_map_json_with_dims(
                    &format!("zakaz-{number}"),
                    &format!("Batch {number}"),
                    number,
                    "7 ta rangli pechat",
                    7.0,
                    650.0,
                ),
            ))
            .await
            .expect("save");
        assert_eq!(save.status(), StatusCode::OK);
    }

    let bad_batch = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"7 ta rangli pechat",
                "to_apparatus":"8 ta rangli pechat",
                "map_ids":["zakaz-1010","zakaz-missing"]
            }"#,
        ))
        .await
        .expect("batch");
    assert_eq!(bad_batch.status(), StatusCode::NOT_FOUND);

    let verify = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    for map in json_body(verify).await.as_array().expect("maps") {
        let apparatus = map["map"]["nodes"]
            .as_array()
            .and_then(|nodes| {
                nodes
                    .iter()
                    .find_map(|node| (node["kind"] == "apparatus").then(|| node["title"].as_str()))
            })
            .flatten()
            .unwrap_or("");
        assert_eq!(apparatus, "7 ta rangli pechat");
    }

    let ok_batch = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"7 ta rangli pechat",
                "to_apparatus":"8 ta rangli pechat",
                "map_ids":["zakaz-1010","zakaz-2020"]
            }"#,
        ))
        .await
        .expect("batch ok");
    assert_eq!(ok_batch.status(), StatusCode::OK);
    assert_eq!(
        json_body(ok_batch).await["saved"]
            .as_array()
            .map(|v| v.len()),
        Some(2)
    );
}

#[tokio::test]
async fn production_map_batch_move_stress_moves_many_orders_atomically() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    for index in 0..24 {
        let number = format!("{index:04}");
        let save = build_router(state.clone())
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &token,
                &pechat_order_map_json_with_dims(
                    &format!("zakaz-{number}"),
                    &format!("Stress {number}"),
                    &number,
                    "7 ta rangli pechat",
                    7.0,
                    650.0,
                ),
            ))
            .await
            .expect("save");
        assert_eq!(save.status(), StatusCode::OK);
    }

    let map_ids: Vec<String> = (0..24)
        .map(|index| format!("\"zakaz-{index:04}\""))
        .collect();
    let body = format!(
        r#"{{"from_apparatus":"7 ta rangli pechat","to_apparatus":"8 ta rangli pechat","map_ids":[{}]}}"#,
        map_ids.join(",")
    );
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            &body,
        ))
        .await
        .expect("stress batch");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["saved"]
            .as_array()
            .map(|v| v.len()),
        Some(24)
    );
}
