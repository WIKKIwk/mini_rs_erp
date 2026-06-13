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
use crate::core::profile::ports::{
    CustomerProfileRecord, DownloadedFile, ProfileLookup, ProfilePortError, SupplierProfileRecord,
};
use crate::core::profile::service::ProfileService;
use crate::core::session::manager::SessionManager;

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: String::new(),
        http_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: unique_profile_store_path(),
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
    });
    state.sessions = SessionManager::memory(Some(30 * 24 * 60 * 60));
    state
}

#[tokio::test]
async fn profile_get_requires_auth_like_go() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/profile")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(response).await["error"], "unauthorized");
}

#[tokio::test]
async fn profile_put_updates_nickname_and_session_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;
    let app = build_router(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/mobile/profile")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"nickname":"Alias"}"#))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["display_name"], "Alias");

    let me = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/me")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(json_body(me).await["display_name"], "Alias");
}

#[tokio::test]
async fn profile_rejects_wrong_method_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/profile")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn profile_avatar_rejects_non_post_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/profile/avatar")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn profile_avatar_requires_multipart_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/profile/avatar")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from("not multipart"))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid multipart");
}

#[tokio::test]
async fn profile_avatar_upload_returns_proxied_supplier_avatar_like_go() {
    let mut state = test_state();
    state.profiles =
        ProfileService::new("http://erp.test".to_string()).with_erp_lookup(Arc::new(FakeLookup));
    let token = supplier_session(&state).await;
    let boundary = "BOUNDARY";
    let body = concat!(
        "--BOUNDARY\r\n",
        "Content-Disposition: form-data; name=\"avatar\"; filename=\"avatar.png\"\r\n",
        "Content-Type: image/png\r\n",
        "\r\n",
        "pngdata\r\n",
        "--BOUNDARY--\r\n"
    );

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/profile/avatar")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::HOST, "mobile.test")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["avatar_url"],
        format!("http://mobile.test/v1/mobile/profile/avatar/view?token={token}")
    );
}

#[tokio::test]
async fn avatar_view_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/profile/avatar/view")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn avatar_view_forbids_non_supplier() {
    let state = test_state();
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Customer,
            display_name: "Customer".to_string(),
            legal_name: "Customer".to_string(),
            ref_: "CUST-001".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session");
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/profile/avatar/view")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn avatar_view_accepts_token_query_with_any_method_like_go() {
    let mut state = test_state();
    state.profiles =
        ProfileService::new("http://erp.test".to_string()).with_erp_lookup(Arc::new(FakeLookup));
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Supplier,
            display_name: "Supplier".to_string(),
            legal_name: "Supplier".to_string(),
            ref_: "SUP-001".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: "http://erp.test/files/uploaded.png".to_string(),
        })
        .await
        .expect("session");

    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/mobile/profile/avatar/view?token={token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(&bytes[..], b"png");
}

async fn supplier_session(state: &AppState) -> String {
    state
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
        .expect("session")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

fn unique_profile_store_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "accord-profile-route-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ))
}

struct FakeLookup;

#[async_trait]
impl ProfileLookup for FakeLookup {
    async fn get_supplier_profile(
        &self,
        _id: &str,
    ) -> Result<SupplierProfileRecord, ProfilePortError> {
        Ok(SupplierProfileRecord {
            phone: "+998901234567".to_string(),
            image: String::new(),
        })
    }

    async fn get_customer_profile(
        &self,
        _id: &str,
    ) -> Result<CustomerProfileRecord, ProfilePortError> {
        Ok(CustomerProfileRecord {
            phone: "+998901234568".to_string(),
        })
    }

    async fn download_file(&self, _file_url: &str) -> Result<DownloadedFile, ProfilePortError> {
        Ok(DownloadedFile {
            content_type: "image/png".to_string(),
            body: b"png".to_vec(),
        })
    }

    async fn upload_supplier_image(
        &self,
        supplier_id: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<String, ProfilePortError> {
        assert_eq!(supplier_id, "SUP-001");
        assert_eq!(filename, "avatar.png");
        assert_eq!(content_type, "image/png");
        assert_eq!(content, b"pngdata".to_vec());
        Ok("/files/uploaded.png".to_string())
    }
}
