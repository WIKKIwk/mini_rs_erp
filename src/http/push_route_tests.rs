use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::push::models::PushTokenRecord;
use crate::core::push::ports::{PushStoreError, PushTokenStorePort};
use crate::core::push::service::PushService;
use crate::core::session::manager::SessionManager;

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: String::new(),
        http_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: "data/mobile_profile_prefs.json".into(),
        push_token_store_path: unique_path(),
        session_ttl_seconds: Some(30 * 24 * 60 * 60),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: "20ABCDEF1234".to_string(),
        werka_name: "Werka".to_string(),
        werka_phone: "+99888862440".to_string(),
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
    });
    state.sessions = SessionManager::memory(Some(30 * 24 * 60 * 60));
    state
}

#[tokio::test]
async fn push_token_requires_auth_like_go() {
    let response = build_router(test_state())
        .oneshot(request("POST", "/v1/mobile/push/token", "", "{}"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(response).await["error"], "unauthorized");
}

#[tokio::test]
async fn push_token_forbids_customer_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Customer, "CUST-001").await;
    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/push/token",
            &token,
            r#"{"token":"device","platform":"ios"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn push_token_registers_supplier_token_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;
    let response = build_router(state.clone())
        .oneshot(request(
            "POST",
            "/v1/mobile/push/token",
            &token,
            r#"{"token":" device-1 ","platform":" ios "}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["ok"], true);
    let records = state
        .push
        .store_for_tests()
        .list("supplier:SUP-001")
        .await
        .expect("list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].token, "device-1");
    assert_eq!(records[0].platform, "ios");
}

#[tokio::test]
async fn push_token_register_requires_body_token_like_go() {
    let mut state = test_state();
    state.push = PushService::new(Arc::new(FailingPushStore));
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;
    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/push/token",
            &token,
            r#"{"token":"   ","platform":"ios"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "token is required");
}

#[tokio::test]
async fn push_token_register_store_failure_uses_save_error() {
    let mut state = test_state();
    state.push = PushService::new(Arc::new(FailingPushStore));
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;
    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/push/token",
            &token,
            r#"{"token":"device-1","platform":"ios"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"], "push token save failed");
}

#[tokio::test]
async fn push_token_delete_requires_query_token_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;
    let response = build_router(state)
        .oneshot(request("DELETE", "/v1/mobile/push/token", &token, ""))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "token is required");
}

#[tokio::test]
async fn push_token_delete_store_failure_uses_delete_error() {
    let mut state = test_state();
    state.push = PushService::new(Arc::new(FailingPushStore));
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;
    let response = build_router(state)
        .oneshot(request(
            "DELETE",
            "/v1/mobile/push/token?token=device-1",
            &token,
            "",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "push token delete failed"
    );
}

#[tokio::test]
async fn push_token_rejects_wrong_method_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;
    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/push/token", &token, ""))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

async fn session(state: &AppState, role: PrincipalRole, ref_: &str) -> String {
    state
        .sessions
        .create(Principal {
            role,
            display_name: "User".to_string(),
            legal_name: "User".to_string(),
            ref_: ref_.to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

fn request(method: &str, uri: &str, token: &str, body: &str) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if !token.is_empty() {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).expect("request")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

fn unique_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "accord-push-route-{}-{}.json",
        std::process::id(),
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ))
}

struct FailingPushStore;

#[async_trait]
impl PushTokenStorePort for FailingPushStore {
    async fn move_token_to_key(
        &self,
        _target_key: &str,
        _token: &str,
        _platform: &str,
    ) -> Result<(), PushStoreError> {
        Err(PushStoreError::StoreFailed)
    }

    async fn delete(&self, _key: &str, _token: &str) -> Result<(), PushStoreError> {
        Err(PushStoreError::StoreFailed)
    }

    async fn list(&self, _key: &str) -> Result<Vec<PushTokenRecord>, PushStoreError> {
        Err(PushStoreError::StoreFailed)
    }
}
