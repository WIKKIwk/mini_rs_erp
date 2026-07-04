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
    DispatchRecord, SupplierDirectoryEntry, WerkaArchiveResponse, WerkaArchiveSummary,
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
async fn werka_archive_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/archive?kind=sent&period=monthly")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_archive_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/archive?kind=sent&period=monthly")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_archive_returns_provider_payload() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/archive?kind=%20sent%20&period=%20monthly%20&from=2026-01-16&to=2026-01-20")
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
    assert_eq!(value["kind"], "sent");
    assert_eq!(value["period"], "monthly");
    assert_eq!(value["from"], "2026-01-16");
    assert_eq!(value["to"], "2026-01-20");
    assert_eq!(value["summary"]["record_count"], 1);
}

#[tokio::test]
async fn werka_archive_invalid_date_is_500_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/archive?from=16-01-2026")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn werka_archive_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/archive?kind=sent&period=monthly")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn werka_archive_pdf_requires_auth() {
    let app = build_router(test_state());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/archive/pdf?kind=sent&period=monthly")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn werka_archive_pdf_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/archive/pdf?kind=sent&period=monthly")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(value["error"], "werka archive pdf failed");
}

#[tokio::test]
async fn werka_archive_pdf_returns_pdf_with_go_headers() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/archive/pdf?kind=sent&period=monthly&from=2026-01-16&to=2026-01-20")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
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
            .expect("content-type"),
        "application/pdf"
    );
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .expect("content-disposition"),
        "attachment; filename=\"werka-sent-monthly.pdf\""
    );
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    assert!(bytes.starts_with(b"%PDF-1.4\n"));
    assert!(bytes.ends_with(b"%%EOF\n"));
}

#[tokio::test]
async fn werka_archive_pdf_accepts_post_like_go_handler() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeWerkaLookup));
    let token = werka_session(&state).await;
    let app = build_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/werka/archive/pdf?kind=sent&period=monthly")
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
        kind: &str,
        period: &str,
        from: Option<Date>,
        to: Option<Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        assert_eq!(kind, "sent");
        assert_eq!(period, "monthly");
        Ok(WerkaArchiveResponse {
            kind: kind.to_string(),
            period: period.to_string(),
            from: from.map(|date| date.to_string()).unwrap_or_default(),
            to: to.map(|date| date.to_string()).unwrap_or_default(),
            summary: WerkaArchiveSummary {
                record_count: 1,
                totals_by_uom: Vec::new(),
            },
            items: vec![DispatchRecord {
                id: "DN-001".to_string(),
                record_type: "delivery_note".to_string(),
                supplier_name: "Customer".to_string(),
                item_code: "ITEM-001".to_string(),
                item_name: "Item".to_string(),
                uom: "Kg".to_string(),
                sent_qty: 12.0,
                accepted_qty: 10.0,
                status: "partial".to_string(),
                created_label: "2026-01-16".to_string(),
                ..DispatchRecord::default()
            }],
        })
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
