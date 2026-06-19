use super::*;

#[tokio::test]
async fn production_map_duplicate_order_number_returns_structured_error() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let first = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json("zakaz-1234", "Old zakaz", "1234", "8 ta rangli pechat"),
        ))
        .await
        .expect("first save");
    assert_eq!(first.status(), StatusCode::OK);

    let duplicate = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json("zakaz-new", "New zakaz", "1234", "8 ta rangli pechat"),
        ))
        .await
        .expect("duplicate save");
    assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(duplicate).await["error"],
        "duplicate_order_number"
    );
}

#[tokio::test]
async fn production_map_rejects_laminatsiya_when_rubber_above_1050() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &laminatsiya_order_map_json("zakaz-lamin-1051", 1051.0),
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await["error"],
        "laminatsiya_rubber_too_large"
    );
}

#[tokio::test]
async fn production_map_allows_laminatsiya_at_1050_rubber() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &laminatsiya_order_map_json("zakaz-lamin-1050", 1050.0),
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn production_map_move_validates_pechat_rules_on_server() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let saved = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json(
                "zakaz-9001",
                "Nine color rubber order",
                "9001",
                "8 ta rangli pechat",
            ),
        ))
        .await
        .expect("save");
    assert_eq!(saved.status(), StatusCode::OK);

    let blocked = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move",
            &token,
            r#"{
                "map_id":"zakaz-9001",
                "from_apparatus":"8 ta rangli pechat",
                "to_apparatus":"7 ta rangli pechat"
            }"#,
        ))
        .await
        .expect("blocked move");
    assert_eq!(blocked.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(blocked).await["error"], "move_not_allowed");

    let allowed = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/move",
            &token,
            r#"{
                "map_id":"zakaz-9001",
                "from_apparatus":"8 ta rangli pechat",
                "to_apparatus":"9 ta rangli pechat"
            }"#,
        ))
        .await
        .expect("allowed move");
    assert_eq!(allowed.status(), StatusCode::OK);
    let body = json_body(allowed).await;
    assert_eq!(body["ok"], true);
    assert_eq!(
        body["saved"]["map"]["nodes"][1]["title"],
        "9 ta rangli pechat"
    );

    let list = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/production-maps", &token))
        .await
        .expect("list");
    let maps = json_body(list).await;
    assert_eq!(maps[0]["map"]["nodes"][1]["title"], "9 ta rangli pechat");
}
