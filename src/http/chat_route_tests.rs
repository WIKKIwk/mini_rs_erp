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

#[tokio::test]
async fn chat_media_upload_initialization_requires_authenticated_session() {
    let response = build_router(test_state())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/chat/conversations/conversation_1/media/uploads")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"client_upload_id":"upload_1","kind":"image","filename":"photo.jpg","content_type":"image/jpeg","size_bytes":3}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn private_chat_media_content_requires_authenticated_session() {
    let response = build_router(test_state())
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/chat/media/media_1/content")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn private_chat_media_head_supports_video_metadata_probes() {
    let response = build_router(test_state())
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/v1/mobile/chat/media/media_1/content")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn resumable_chat_media_chunk_requires_authenticated_session() {
    let response = build_router(test_state())
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/mobile/chat/conversations/conversation_1/media/uploads/upload_1/chunks/0")
                .header(header::CONTENT_LENGTH, "3")
                .header(header::CONTENT_RANGE, "bytes 0-2/3")
                .body(Body::from("123"))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn chat_media_upload_initialization_rejects_oversized_images_before_storage() {
    let mut state = test_state();
    state.chat_media = crate::core::chat_media::ChatMediaService::unavailable();
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
                .uri("/v1/mobile/chat/conversations/conversation_1/media/uploads")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"client_upload_id":"upload_1","kind":"image","filename":"photo.jpg","content_type":"image/jpeg","size_bytes":15728641}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["error"],
        "chat_media_too_large"
    );
}

#[tokio::test]
async fn chat_media_upload_initialization_rejects_video_over_two_gibibytes() {
    let mut state = test_state();
    state.chat_media = crate::core::chat_media::ChatMediaService::unavailable();
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
                .uri("/v1/mobile/chat/conversations/conversation_1/media/uploads")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"client_upload_id":"upload_1","kind":"video","filename":"incident.mp4","content_type":"video/mp4","size_bytes":2147483649,"duration_ms":600000}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["error"],
        "chat_media_too_large"
    );
}

#[tokio::test]
async fn chat_media_upload_initialization_rejects_video_over_ten_minutes() {
    let mut state = test_state();
    state.chat_media = crate::core::chat_media::ChatMediaService::unavailable();
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
                .uri("/v1/mobile/chat/conversations/conversation_1/media/uploads")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"client_upload_id":"upload_1","kind":"video","filename":"incident.mp4","content_type":"video/mp4","size_bytes":1024,"duration_ms":600001}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["error"],
        "video_duration_too_long"
    );
}

#[tokio::test]
async fn chat_media_upload_initialization_rejects_audio_over_sixty_four_mebibytes() {
    let mut state = test_state();
    state.chat_media = crate::core::chat_media::ChatMediaService::unavailable();
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
                .uri("/v1/mobile/chat/conversations/conversation_1/media/uploads")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"client_upload_id":"upload_1","kind":"audio","filename":"voice.m4a","content_type":"audio/mp4","size_bytes":67108865,"duration_ms":1000}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["error"],
        "chat_media_too_large"
    );
}

#[tokio::test]
async fn chat_media_upload_initialization_rejects_audio_over_ten_minutes() {
    let mut state = test_state();
    state.chat_media = crate::core::chat_media::ChatMediaService::unavailable();
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
                .uri("/v1/mobile/chat/conversations/conversation_1/media/uploads")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"client_upload_id":"upload_1","kind":"audio","filename":"voice.m4a","content_type":"audio/mp4","size_bytes":1024,"duration_ms":600001}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["error"],
        "audio_duration_too_long"
    );
}

#[tokio::test]
async fn chat_media_upload_initialization_rejects_unknown_media_kind() {
    let state = test_state();
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
                .uri("/v1/mobile/chat/conversations/conversation_1/media/uploads")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"client_upload_id":"upload_1","kind":"document","filename":"file.bin","content_type":"application/octet-stream","size_bytes":3}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
