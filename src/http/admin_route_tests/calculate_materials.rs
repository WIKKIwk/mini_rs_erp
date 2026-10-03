use super::*;

#[tokio::test]
async fn calculate_material_sequence_is_admin_only_and_validates_visible_ids() {
    let mut state = test_state();
    state.calculate_materials =
        Arc::new(crate::core::calculate_materials::MemoryCalculateMaterialStore::new());
    let admin = session(&state, PrincipalRole::Admin).await;
    let supplier = session(&state, PrincipalRole::Supplier).await;
    let ids: Vec<_> = state
        .calculate_materials
        .list()
        .await
        .unwrap()
        .into_iter()
        .rev()
        .map(|material| material.id)
        .collect();
    let body = serde_json::json!({"material_ids": ids}).to_string();
    let router = build_router(state.clone());
    let route = "/v1/mobile/admin/calculate-materials/sequence";
    let denied = router
        .clone()
        .oneshot(request_with_body("PUT", route, &supplier, &body))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let response = router
        .clone()
        .oneshot(request_with_body("PUT", route, &admin, &body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let saved = json_body(response).await;
    let listed = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/calculate-materials",
            &admin,
        ))
        .await
        .unwrap();
    assert_eq!(json_body(listed).await["materials"], saved["materials"]);
    assert_eq!(saved["materials"][0]["id"], ids[0]);
    let before = state.calculate_materials.list().await.unwrap();
    let invalid = router
        .oneshot(request_with_body(
            "PUT",
            route,
            &admin,
            r#"{"material_ids":["unknown"]}"#,
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(state.calculate_materials.list().await.unwrap(), before);
}
