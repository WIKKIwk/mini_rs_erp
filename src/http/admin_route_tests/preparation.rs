use super::*;

#[tokio::test]
async fn preparation_system_role_create_login_list_scope_and_fail_closed() {
    let mut state = test_state();
    state.preparation = None;
    let admin = session(&state, PrincipalRole::Admin).await;
    let created=build_router(state.clone()).oneshot(request_with_body("POST","/v1/mobile/admin/system-users",&admin,
        r#"{"id":"prep_test","role":"tayyorlov_masteri","name":"Tayyorlov masteri","phone":"+998901112290"}"#)).await.unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let code = build_router(state.clone())
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/system-users/code/regenerate?id=prep_test",
            &admin,
        ))
        .await
        .unwrap();
    assert_eq!(code.status(), StatusCode::OK);
    let code = json_body(code).await["code"].as_str().unwrap().to_string();
    assert!(code.starts_with("90"));
    let login = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            &format!(r#"{{"phone":"+998901112290","code":"{code}"}}"#),
        ))
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    let body = json_body(login).await;
    assert_eq!(body["profile"]["role"], "tayyorlov_masteri");
    assert_eq!(
        body["capabilities"],
        serde_json::json!(["preparation.access"])
    );
    let token = body["token"].as_str().unwrap();
    let list = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=tayyorlov_masteri",
            &admin,
        ))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    assert_eq!(
        json_body(list).await["items"][0]["principal_role"],
        "tayyorlov_masteri"
    );
    let missing = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/preparation/snapshot", token))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::SERVICE_UNAVAILABLE);
    for role in [
        PrincipalRole::MaterialTaminotchi,
        PrincipalRole::Boyoqchi,
        PrincipalRole::Customer,
        PrincipalRole::Admin,
    ] {
        let token = session(&state, role).await;
        let denied = build_router(state.clone())
            .oneshot(request("GET", "/v1/mobile/preparation/snapshot", &token))
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    }
    let denied = build_router(state)
        .oneshot(request("GET", "/v1/mobile/preparation/snapshot", ""))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}
