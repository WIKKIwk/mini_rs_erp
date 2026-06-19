use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::werka::service::WerkaService;
use crate::http::router::build_router;

use super::support::{FakeWerkaHomeLookup, json_body, test_state, werka_session};

#[tokio::test]
async fn werka_home_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/home")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_home_forbids_non_werka() {
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
                .uri("/v1/mobile/werka/home")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn werka_home_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/home")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_home_returns_provider_payload() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/home")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["summary"]["pending_count"], 2);
    assert_eq!(value["summary"]["confirmed_count"], 3);
    assert_eq!(value["summary"]["returned_count"], 1);
    assert_eq!(value["pending_items"], serde_json::json!([]));
}

#[tokio::test]
async fn werka_home_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/home")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn werka_summary_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/summary")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_summary_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/summary")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_summary_returns_provider_payload() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/summary")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(
        value,
        serde_json::json!({
            "pending_count": 2,
            "confirmed_count": 3,
            "returned_count": 1
        })
    );
}

#[tokio::test]
async fn werka_summary_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/summary")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn werka_pending_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/pending")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_pending_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/pending")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_pending_returns_provider_payload() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/pending")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value[0]["id"], "PR-001");
    assert_eq!(value[0]["status"], "pending");
}

#[tokio::test]
async fn werka_pending_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaHomeLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/pending")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}
