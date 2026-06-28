use super::*;

#[tokio::test]
async fn production_map_batch_move_allows_seven_to_eight_color_pechat() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json_with_dims(
                "zakaz-3030",
                "Dual pechat order",
                "3030",
                "7 ta rangli pechat - A",
                7.0,
                650.0,
            ),
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
                "from_apparatus":"7 ta rangli pechat",
                "to_apparatus":"8 ta rangli pechat",
                "map_ids":["zakaz-3030"]
            }"#,
        ))
        .await
        .expect("batch move");
    assert_eq!(moved.status(), StatusCode::OK);

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    let apparatus = maps[0]["map"]["nodes"]
        .as_array()
        .and_then(|nodes| {
            nodes
                .iter()
                .find_map(|node| (node["kind"] == "apparatus").then(|| node["title"].as_str()))
        })
        .flatten()
        .unwrap_or("");
    assert_eq!(apparatus, "8 ta rangli pechat");
}

#[tokio::test]
async fn production_map_batch_move_blocks_flexo_order_to_color_pechat() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &production_order_map_json_with_product(
                "zakaz-flexo-3031",
                "vitagum flexo zip paket",
                "FLEXO-3031",
                "3031",
                "Flexo pechat - A",
                7.0,
                650.0,
            ),
        ))
        .await
        .expect("save");
    assert_eq!(save.status(), StatusCode::OK);

    let moved = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move-batch",
            &token,
            r#"{
                "from_apparatus":"Flexo pechat - A",
                "to_apparatus":"8 ta rangli pechat",
                "map_ids":["zakaz-flexo-3031"]
            }"#,
        ))
        .await
        .expect("batch move");
    assert_eq!(moved.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(moved).await["error"], "move_not_allowed");
}

#[tokio::test]
async fn production_map_batch_move_reassigns_alternative_apparatus_assignment() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-alt-move",
                "product_code":"ALT-MOVE",
                "title":"Alternative move order",
                "roll_count":7,
                "width_mm":650,
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {
                        "id":"apparatus-7",
                        "kind":"apparatus",
                        "title":"7 ta rangli pechat",
                        "alternative_group_id":"alt-pechat",
                        "alternative_group_label":"pechat",
                        "alternative_assigned_title":"7 ta rangli pechat"
                    },
                    {
                        "id":"apparatus-8",
                        "kind":"apparatus",
                        "title":"8 ta rangli pechat",
                        "alternative_group_id":"alt-pechat",
                        "alternative_group_label":"pechat",
                        "alternative_assigned_title":"7 ta rangli pechat"
                    },
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"apparatus-7"},
                    {"from":"apparatus-7","to":"end"},
                    {"from":"start","to":"apparatus-8"},
                    {"from":"apparatus-8","to":"end"}
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
                "from_apparatus":"7 ta rangli pechat",
                "to_apparatus":"8 ta rangli pechat",
                "map_ids":["zakaz-alt-move"]
            }"#,
        ))
        .await
        .expect("batch move");
    assert_eq!(moved.status(), StatusCode::OK);

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    let nodes = maps[0]["map"]["nodes"].as_array().expect("nodes");
    let apparatus_titles: Vec<&str> = nodes
        .iter()
        .filter(|node| node["kind"] == "apparatus")
        .filter_map(|node| node["title"].as_str())
        .collect();
    let assigned_titles: Vec<&str> = nodes
        .iter()
        .filter(|node| node["kind"] == "apparatus")
        .filter_map(|node| node["alternative_assigned_title"].as_str())
        .collect();
    assert_eq!(
        apparatus_titles,
        vec!["7 ta rangli pechat", "8 ta rangli pechat"]
    );
    assert_eq!(
        assigned_titles,
        vec!["8 ta rangli pechat", "8 ta rangli pechat"]
    );
}

#[tokio::test]
async fn production_map_batch_move_preserves_alternative_node_titles_when_target_is_absent() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-alt-title-preserve",
                "product_code":"ALT-TITLE",
                "title":"Alternative title preserve order",
                "roll_count":7,
                "width_mm":630,
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {
                        "id":"apparatus-7-a",
                        "kind":"apparatus",
                        "title":"7 ta rangli pechat - A",
                        "alternative_group_id":"alt-pechat",
                        "alternative_group_label":"pechat",
                        "alternative_assigned_title":"7 ta rangli pechat - A"
                    },
                    {
                        "id":"apparatus-7-b",
                        "kind":"apparatus",
                        "title":"7 ta rangli pechat - A",
                        "alternative_group_id":"alt-pechat",
                        "alternative_group_label":"pechat",
                        "alternative_assigned_title":"7 ta rangli pechat - A"
                    },
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"apparatus-7-a"},
                    {"from":"apparatus-7-a","to":"end"},
                    {"from":"start","to":"apparatus-7-b"},
                    {"from":"apparatus-7-b","to":"end"}
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
                "from_apparatus":"7 ta rangli pechat - A",
                "to_apparatus":"8 ta rangli pechat - A",
                "map_ids":["zakaz-alt-title-preserve"]
            }"#,
        ))
        .await
        .expect("batch move");
    assert_eq!(moved.status(), StatusCode::OK);

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    let nodes = maps[0]["map"]["nodes"].as_array().expect("nodes");
    let apparatus_titles: Vec<&str> = nodes
        .iter()
        .filter(|node| node["kind"] == "apparatus")
        .filter_map(|node| node["title"].as_str())
        .collect();
    let assigned_titles: Vec<&str> = nodes
        .iter()
        .filter(|node| node["kind"] == "apparatus")
        .filter_map(|node| node["alternative_assigned_title"].as_str())
        .collect();
    assert_eq!(
        apparatus_titles,
        vec!["7 ta rangli pechat - A", "7 ta rangli pechat - A"]
    );
    assert_eq!(
        assigned_titles,
        vec!["8 ta rangli pechat - A", "8 ta rangli pechat - A"]
    );
}
