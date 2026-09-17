use super::*;

#[tokio::test]
async fn telegram_qr_routes_require_settings_management() {
    let state = test_state();
    let worker = session(&state, PrincipalRole::Werka).await;
    for (method, url) in [
        ("POST", "/v1/mobile/admin/telegram/qr-logins"),
        ("PUT", "/v1/mobile/admin/telegram/userbot-settings"),
        ("GET", "/v1/mobile/admin/telegram/qr-logins/challenge"),
        ("POST", "/v1/mobile/admin/telegram/qr-logins/challenge"),
        ("DELETE", "/v1/mobile/admin/telegram/qr-logins/challenge"),
    ] {
        let response = build_router(state.clone()).oneshot(request(method, url, &worker)).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = build_router(state.clone()).oneshot(request(method, url, "invalid-token")).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn telegram_qr_unknown_challenge_does_not_authorize_user() {
    let state = test_state();
    let admin = session(&state, PrincipalRole::Admin).await;
    let response = build_router(state).oneshot(request("GET", "/v1/mobile/admin/telegram/qr-logins/missing", &admin)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "expired");
}

#[tokio::test]
async fn telegram_user_delete_route_requires_an_existing_user() {
    let state = test_state();
    let admin = session(&state, PrincipalRole::Admin).await;
    let response = build_router(state)
        .oneshot(request(
            "DELETE",
            "/v1/mobile/admin/telegram/users/missing",
            &admin,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await["error"],
        "telegram user account is not connected"
    );
}
