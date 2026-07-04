use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use time::Date;
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::manager::SessionManager;
use crate::core::werka::models::{
    CustomerDirectoryEntry, DispatchRecord, SupplierDirectoryEntry, WerkaArchiveResponse,
    WerkaHomeData, WerkaHomeSummary, WerkaStatusBreakdownEntry,
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
        session_ttl_seconds: Some(30 * 24 * 60 * 60),
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
    state.sessions = SessionManager::memory(Some(30 * 24 * 60 * 60));
    state
}

#[tokio::test]
async fn werka_suppliers_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/suppliers")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_suppliers_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/suppliers")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_suppliers_returns_provider_payload_and_parses_query() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/suppliers?q=%20Ali%20&limit=999&offset=3")
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
    assert_eq!(value[0]["ref"], "SUP-001");
    assert_eq!(value[0]["name"], "Ali");
}

#[tokio::test]
async fn werka_suppliers_defaults_invalid_limit_and_offset_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(DefaultLimitLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/suppliers?limit=abc&offset=-9")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn werka_suppliers_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/suppliers?q=Ali&limit=200&offset=3")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn werka_customers_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/customers")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_customers_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/customers")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_customers_returns_provider_payload_and_parses_query() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/customers?q=%20Ali%20&limit=999&offset=3")
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
    assert_eq!(value[0]["ref"], "CUST-001");
    assert_eq!(value[0]["name"], "Ali Market");
}

#[tokio::test]
async fn werka_customers_defaults_invalid_limit_and_offset_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(DefaultLimitLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/customers?limit=abc&offset=-9")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn werka_customers_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/customers?q=Ali&limit=200&offset=3")
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
struct DefaultLimitLookup;

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
        Ok(Vec::new())
    }

    async fn werka_status_breakdown(
        &self,
        _kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        Ok(Vec::new())
    }

    async fn werka_status_details(
        &self,
        _kind: &str,
        _supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }

    async fn werka_archive(
        &self,
        _kind: &str,
        _period: &str,
        _from: Option<Date>,
        _to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        Ok(WerkaArchiveResponse::default())
    }

    async fn werka_suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError> {
        assert_eq!(query, "Ali");
        assert_eq!(limit, 200);
        assert_eq!(offset, 3);
        Ok(vec![SupplierDirectoryEntry {
            ref_: "SUP-001".to_string(),
            name: "Ali".to_string(),
            phone: "+998901111111".to_string(),
        }])
    }

    async fn werka_customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, WerkaPortError> {
        assert_eq!(query, "Ali");
        assert_eq!(limit, 200);
        assert_eq!(offset, 3);
        Ok(vec![CustomerDirectoryEntry {
            ref_: "CUST-001".to_string(),
            name: "Ali Market".to_string(),
            phone: "+998902222222".to_string(),
        }])
    }
}

#[async_trait]
impl WerkaHomeLookup for DefaultLimitLookup {
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
        Ok(Vec::new())
    }

    async fn werka_status_breakdown(
        &self,
        _kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        Ok(Vec::new())
    }

    async fn werka_status_details(
        &self,
        _kind: &str,
        _supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }

    async fn werka_archive(
        &self,
        _kind: &str,
        _period: &str,
        _from: Option<Date>,
        _to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        Ok(WerkaArchiveResponse::default())
    }

    async fn werka_suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError> {
        assert_eq!(query, "");
        assert_eq!(limit, 200);
        assert_eq!(offset, 0);
        Ok(Vec::new())
    }

    async fn werka_customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, WerkaPortError> {
        assert_eq!(query, "");
        assert_eq!(limit, 200);
        assert_eq!(offset, 0);
        Ok(Vec::new())
    }
}
