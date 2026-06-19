use super::*;

#[tokio::test]
async fn admin_item_group_tree_returns_parent_shape() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/item-groups/tree", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value[0]["name"], "All Item Groups");
    assert_eq!(value[1]["name"], "Xomashyo");
    assert_eq!(value[1]["parent_item_group"], "All Item Groups");
    assert_eq!(value[2]["name"], "plyonka");
    assert_eq!(value[2]["parent_item_group"], "Xomashyo");
}

#[tokio::test]
async fn admin_item_group_create_returns_item_group_shape() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let group = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"Kley","parent":"Kraska","is_group":false}"#,
        ))
        .await
        .expect("response");
    assert_eq!(group.status(), StatusCode::OK);
    let value = json_body(group).await;
    assert_eq!(value["name"], "Kley");
    assert_eq!(value["item_group_name"], "Kley");
    assert_eq!(value["parent_item_group"], "Kraska");
    assert_eq!(value["is_group"], false);

    let invalid = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"","parent":"All Item Groups","is_group":true}"#,
        ))
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid).await["error"],
        "item group name is required"
    );
}

#[tokio::test]
async fn admin_item_group_parent_move_returns_item_group_shape() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let moved = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"Xomashyo","parent":"All Item Groups"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(moved.status(), StatusCode::OK);
    let value = json_body(moved).await;
    assert_eq!(value["name"], "Xomashyo");
    assert_eq!(value["item_group_name"], "Xomashyo");
    assert_eq!(value["parent_item_group"], "All Item Groups");
    assert_eq!(value["is_group"], true);

    let invalid_root = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"All Item Groups","parent":"Products"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(invalid_root.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid_root).await["error"],
        "root item group cannot be moved"
    );

    let invalid_cycle = build_router(state)
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/item-groups",
            &token,
            r#"{"name":"Xomashyo","parent":"Xomashyo"}"#,
        ))
        .await
        .expect("response");
    assert_eq!(invalid_cycle.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid_cycle).await["error"],
        "item group cannot be its own parent"
    );
}
