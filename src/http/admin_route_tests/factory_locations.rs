use super::*;

static FACTORY_LOCATION_ROUTE_TEST_LOCK: Mutex<()> = Mutex::const_new(());

#[tokio::test]
async fn admin_factory_location_flow_keeps_identity_when_apparatus_changes() {
    let _guard = FACTORY_LOCATION_ROUTE_TEST_LOCK.lock().await;
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/factory-locations",
            &token,
            r#"{
                "name":"Bosma oldi",
                "apparatus_ids":["apparatus:default:bosma_7"]
            }"#,
        ))
        .await
        .expect("create response");
    assert_eq!(created.status(), StatusCode::OK);
    let created = json_body(created).await;
    let id = created["id"].as_str().expect("state id").to_string();
    assert!(id.starts_with("state_"));
    assert_eq!(created["name"], "Bosma oldi");
    assert_eq!(created["apparatus"][0]["source"], "default");
    assert_eq!(created["apparatus"][0]["id"], "apparatus:default:bosma_7");

    let updated = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            &format!("/v1/mobile/admin/factory-locations/{id}/apparatus"),
            &token,
            r#"{"apparatus_ids":["apparatus:default:asset-010"]}"#,
        ))
        .await
        .expect("apparatus update response");
    assert_eq!(updated.status(), StatusCode::OK);
    let updated = json_body(updated).await;
    assert_eq!(updated["id"], id);
    assert_eq!(updated["name"], "Bosma oldi");
    assert_eq!(updated["apparatus"][0]["id"], "apparatus:default:asset-010");
    assert_eq!(updated["apparatus"][0]["name"], "Rezka");

    let listed = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/factory-locations", &token))
        .await
        .expect("list response");
    assert_eq!(listed.status(), StatusCode::OK);
    assert_eq!(json_body(listed).await.as_array().expect("states").len(), 1);
}

#[tokio::test]
async fn factory_location_rejects_duplicate_name_and_unknown_apparatus() {
    let _guard = FACTORY_LOCATION_ROUTE_TEST_LOCK.lock().await;
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/factory-locations",
            &token,
            r#"{"name":"Laminat oldi"}"#,
        ))
        .await
        .expect("create response");
    assert_eq!(created.status(), StatusCode::OK);

    let duplicate = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/factory-locations",
            &token,
            r#"{"name":" laminat OLDI "}"#,
        ))
        .await
        .expect("duplicate response");
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);

    let invalid = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/factory-locations",
            &token,
            r#"{
                "name":"Noma'lum",
                "apparatus_ids":["apparatus:missing"]
            }"#,
        ))
        .await
        .expect("invalid apparatus response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn factory_location_rejects_display_title_as_apparatus_key() {
    let _guard = FACTORY_LOCATION_ROUTE_TEST_LOCK.lock().await;
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/factory-locations",
            &token,
            r#"{"name":"Title key","apparatus_ids":["Rezka"]}"#,
        ))
        .await
        .expect("invalid title key response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn material_taminotchi_cannot_manage_factory_locations_by_default() {
    let _guard = FACTORY_LOCATION_ROUTE_TEST_LOCK.lock().await;
    let state = test_state();
    let token = session(&state, PrincipalRole::MaterialTaminotchi).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/factory-locations", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
