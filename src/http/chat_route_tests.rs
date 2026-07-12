use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::manager::SessionManager;

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: String::new(),
        http_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: "data/mobile_profiles.json".into(),
        push_token_store_path: "data/mobile_push_tokens.json".into(),
        session_ttl_seconds: Some(3600),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: "20ABCDEF1234".to_string(),
        werka_name: "Werka".to_string(),
        werka_phone: "+99888862440".to_string(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: "Material taminotchisi".to_string(),
        material_taminotchi_phone: String::new(),
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
    });
    state.sessions = SessionManager::memory(Some(3600));
    state
}

#[tokio::test]
async fn chat_directory_requires_authenticated_session() {
    let response = build_router(test_state())
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/chat/directory")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authenticated_user_can_issue_chat_socket_ticket() {
    let state = test_state();
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Aparatchi,
            display_name: "Operator".to_string(),
            legal_name: "Operator".to_string(),
            ref_: "worker_001".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .expect("session");
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/chat/socket-ticket")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert!(value["ticket"].as_str().unwrap_or_default().len() >= 32);
    assert_eq!(value["expires_in_seconds"], 30);
}

#[tokio::test]
async fn customer_can_register_device_token_for_chat() {
    let state = test_state();
    let store = state.push.store_for_tests();
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Customer,
            display_name: "Customer".to_string(),
            legal_name: "Customer".to_string(),
            ref_: "customer_001".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .expect("session");
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/chat/device-token")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"token":"fcm-customer","platform":"ios"}"#))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let records = store.list("customer:customer_001").await.expect("tokens");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].token, "fcm-customer");
}
