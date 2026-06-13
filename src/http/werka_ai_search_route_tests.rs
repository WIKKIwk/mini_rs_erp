use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::manager::SessionManager;
use crate::core::werka::models::WerkaAiSearchSuggestion;
use crate::core::werka::ports::{WerkaAiSearch, WerkaAiSearchError, WerkaAiSearchImage};
use crate::core::werka::service::WerkaService;

#[tokio::test]
async fn ai_search_rejects_non_post_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/werka/ai-search-suggestion")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let value = json_body(response).await;
    assert_eq!(value["error"], "method not allowed");
    assert_eq!(value["code"], "method_not_allowed");
}

#[tokio::test]
async fn ai_search_forbids_non_werka_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;
    let response = build_router(state)
        .oneshot(multipart_request(&token, "image", b"image"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn ai_search_returns_not_configured_before_parsing_upload_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/ai-search-suggestion")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let value = json_body(response).await;
    assert_eq!(value["error"], "werka ai search is not configured");
    assert_eq!(value["code"], "not_configured");
}

#[tokio::test]
async fn ai_search_requires_image_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_ai_search(Arc::new(FakeAiSearch::default()));
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(multipart_request(&token, "other", b"image"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let value = json_body(response).await;
    assert_eq!(value["error"], "image is required");
    assert_eq!(value["code"], "invalid_image");
}

#[tokio::test]
async fn ai_search_returns_suggestion_and_detects_mime() {
    let mut state = test_state();
    let search = Arc::new(FakeAiSearch::default());
    state.werka = WerkaService::new().with_ai_search(search.clone());
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(multipart_request(
            &token,
            "image",
            b"\x89PNG\r\n\x1a\nimage",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["display_query"], "nivea");
    assert_eq!(value["background_queries"][0], "nivea");
    assert_eq!(
        search.calls.lock().expect("calls")[0],
        WerkaAiSearchImage {
            bytes: b"\x89PNG\r\n\x1a\nimage".to_vec(),
            mime_type: "image/png".to_string(),
        }
    );
}

#[tokio::test]
async fn ai_search_no_result_returns_empty_object_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_ai_search(Arc::new(FakeAiSearch {
        result: Err(WerkaAiSearchError::no_result()),
        ..FakeAiSearch::default()
    }));
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(multipart_request(&token, "image", b"image"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        serde_json::json!({
            "display_query": "",
            "background_queries": null,
            "visible_text": "",
        })
    );
}

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        erp_url: String::new(),
        erp_api_key: String::new(),
        erp_api_secret: String::new(),
        default_target_warehouse: String::new(),
        erp_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: "data/mobile_profile_prefs.json".into(),
        push_token_store_path: "data/mobile_push_tokens.json".into(),
        admin_supplier_store_path: "data/mobile_admin_suppliers.json".into(),
        session_ttl_seconds: Some(30 * 24 * 60 * 60),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: "20ABCDEF1234".to_string(),
        werka_name: "Werka".to_string(),
        werka_phone: "+99888862440".to_string(),
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
        direct_read_enabled: false,
        direct_site_config_path: String::new(),
        direct_db_host: String::new(),
        direct_db_port: None,
        direct_db_user: String::new(),
        direct_db_password: String::new(),
        direct_db_name: String::new(),
        catalog_cache_enabled: false,
        catalog_cache_fallback_direct_db: true,
        catalog_cache_path: std::path::PathBuf::from("data/catalog_cache.sqlite"),
    });
    state.sessions = SessionManager::memory(Some(30 * 24 * 60 * 60));
    state
}

async fn werka_session(state: &AppState) -> String {
    session(state, PrincipalRole::Werka, "werka").await
}

async fn supplier_session(state: &AppState) -> String {
    session(state, PrincipalRole::Supplier, "SUP-001").await
}

async fn session(state: &AppState, role: PrincipalRole, ref_: &str) -> String {
    state
        .sessions
        .create(Principal {
            role,
            display_name: "Werka".to_string(),
            legal_name: "Werka".to_string(),
            ref_: ref_.to_string(),
            phone: "+998901111111".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

fn multipart_request(token: &str, field: &str, bytes: &[u8]) -> Request<Body> {
    let boundary = "BOUNDARY";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{field}\"; filename=\"scan.png\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    Request::builder()
        .method("POST")
        .uri("/v1/mobile/werka/ai-search-suggestion")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .expect("request")
}

async fn json_body(response: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("json")
}

struct FakeAiSearch {
    result: Result<WerkaAiSearchSuggestion, WerkaAiSearchError>,
    calls: Mutex<Vec<WerkaAiSearchImage>>,
}

impl Default for FakeAiSearch {
    fn default() -> Self {
        Self {
            result: Ok(WerkaAiSearchSuggestion {
                display_query: "nivea".to_string(),
                background_queries: vec!["nivea".to_string()],
                visible_text: "Nivea Creme Care".to_string(),
            }),
            calls: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl WerkaAiSearch for FakeAiSearch {
    async fn infer_suggestion(
        &self,
        image: WerkaAiSearchImage,
    ) -> Result<WerkaAiSearchSuggestion, WerkaAiSearchError> {
        self.calls.lock().expect("calls").push(image);
        self.result.clone()
    }
}
