use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::http::router::build_router;

use super::support::{json_body, test_state};

#[tokio::test]
async fn auth_login_rejects_non_post_with_json_like_go() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/auth/login")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn auth_login_rejects_werka_code_with_wrong_phone() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"phone":"+998880000000","code":"20ABCDEF1234"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(response).await["error"], "invalid credentials");
}

#[tokio::test]
async fn auth_logout_rejects_non_post_with_json_like_go() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/auth/logout")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn me_accepts_post_like_go() {
    let state = test_state();
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Supplier,
            display_name: "Supplier".to_string(),
            legal_name: "Supplier".to_string(),
            ref_: "SUP-001".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session");
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/me")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["role"], "supplier");
    assert_eq!(value["ref"], "SUP-001");
}
