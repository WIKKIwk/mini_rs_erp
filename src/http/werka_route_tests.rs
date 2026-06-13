use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::manager::SessionManager;
use crate::core::werka::models::{
    DispatchRecord, SupplierDirectoryEntry, WerkaArchiveResponse, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::core::werka::service::WerkaService;

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: String::new(),
        http_timeout: std::time::Duration::from_secs(15),
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
    });
    state.sessions = SessionManager::memory(Some(30 * 24 * 60 * 60));
    state
}

#[tokio::test]
async fn werka_history_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/history")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_history_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/history")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_history_returns_provider_payload() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/history")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(value[0]["id"], "supplier_ack:COMM-001");
    assert_eq!(value[0]["event_type"], "supplier_ack");
}

#[tokio::test]
async fn werka_history_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/history")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn werka_notifications_aliases_history_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/notifications")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(value[0]["id"], "supplier_ack:COMM-001");
}

#[tokio::test]
async fn werka_notifications_accepts_post_like_history() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/notifications")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn werka_status_breakdown_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/status-breakdown?kind=pending")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_status_breakdown_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/status-breakdown?kind=pending")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_status_breakdown_returns_provider_payload() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/status-breakdown?kind=returned")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(value[0]["supplier_ref"], "SUP-001");
    assert_eq!(value[0]["receipt_count"], 1);
}

#[tokio::test]
async fn werka_status_breakdown_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/status-breakdown?kind=returned")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn werka_status_details_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/status-details?kind=pending&supplier_ref=SUP-001")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_status_details_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/status-details?kind=pending&supplier_ref=SUP-001")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_status_details_returns_provider_payload() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri(
                    "/v1/mobile/werka/status-details?kind=%20pending%20&supplier_ref=%20SUP-001%20",
                )
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(value[0]["id"], "PR-001");
    assert_eq!(value[0]["supplier_ref"], "SUP-001");
}

#[tokio::test]
async fn werka_status_details_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/status-details?kind=pending&supplier_ref=SUP-001")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

async fn werka_session(state: &AppState) -> String {
    state
        .sessions
        .create(Principal {
            role: PrincipalRole::Werka,
            display_name: "Werka".to_string(),
            legal_name: "Werka".to_string(),
            ref_: "werka".to_string(),
            phone: "+99888862440".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

struct FakeWerkaLookup;

#[async_trait]
impl WerkaHomeLookup for FakeWerkaLookup {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        Ok(WerkaHomeSummary::default())
    }

    async fn werka_home(&self, _pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        Ok(WerkaHomeData::default())
    }

    async fn werka_pending(&self, _limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }

    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(vec![DispatchRecord {
            id: "supplier_ack:COMM-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 10.0,
            event_type: "supplier_ack".to_string(),
            status: "accepted".to_string(),
            created_label: "2026-01-16".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_status_breakdown(
        &self,
        kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        assert_eq!(kind, "returned");
        Ok(vec![WerkaStatusBreakdownEntry {
            supplier_ref: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            receipt_count: 1,
            total_sent_qty: 10.0,
            total_accepted_qty: 8.0,
            total_returned_qty: 2.0,
            uom: "Kg".to_string(),
        }])
    }

    async fn werka_status_details(
        &self,
        kind: &str,
        supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        assert_eq!(kind, "pending");
        assert_eq!(supplier_ref, "SUP-001");
        Ok(vec![DispatchRecord {
            id: "PR-001".to_string(),
            supplier_ref: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 0.0,
            status: "pending".to_string(),
            created_label: "2026-01-16".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_archive(
        &self,
        _kind: &str,
        _period: &str,
        _from: Option<time::Date>,
        _to: Option<time::Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        Ok(WerkaArchiveResponse::default())
    }

    async fn werka_suppliers(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError> {
        Ok(Vec::new())
    }
}
