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
use crate::core::werka::models::StockEntryBarcodeEntry;
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::core::werka::service::WerkaService;

#[tokio::test]
async fn stock_entry_lookup_requires_auth_like_go() {
    let response = build_router(test_state(Some(Arc::new(FakeStockLookup::found()))))
        .oneshot(request(
            "GET",
            "/v1/mobile/stock-entry/lookup?barcode=30AD",
            "",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(response).await["error"], "unauthorized");
}

#[tokio::test]
async fn stock_entry_lookup_rejects_wrong_method_like_go() {
    let state = test_state(Some(Arc::new(FakeStockLookup::found())));
    let token = session(&state).await;
    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/stock-entry/lookup?barcode=30AD",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn stock_entry_lookup_requires_barcode_like_go() {
    let state = test_state(Some(Arc::new(FakeStockLookup::found())));
    let token = session(&state).await;
    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/stock-entry/lookup", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "barcode is required");
}

#[tokio::test]
async fn stock_entry_lookup_returns_payload_like_go() {
    let state = test_state(Some(Arc::new(FakeStockLookup::found())));
    let token = session(&state).await;
    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/stock-entry/lookup?epc=30ad&limit=5",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["barcode"], "30AD");
    assert_eq!(value["count"], 1);
    assert_eq!(value["entries"][0]["stock_entry_name"], "MAT-STE-001");
    assert_eq!(value["entries"][0]["line_index"], 1);
}

#[tokio::test]
async fn stock_entry_lookup_maps_not_found_and_unavailable_like_go() {
    let state = test_state(Some(Arc::new(FakeStockLookup::not_found())));
    let token = session(&state).await;
    let not_found = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/stock-entry/lookup?barcode=missing",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(not_found.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(not_found).await["error"], "stock entry not found");

    let state = test_state(None);
    let token = session(&state).await;
    let unavailable = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/stock-entry/lookup?barcode=30AD",
            &token,
        ))
        .await
        .expect("response");
    assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        json_body(unavailable).await["error"],
        "direct db lookup unavailable"
    );
}

fn test_state(lookup: Option<Arc<dyn WerkaHomeLookup>>) -> AppState {
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
    if let Some(lookup) = lookup {
        state.werka = WerkaService::new().with_lookup(lookup);
    }
    state
}

async fn session(state: &AppState) -> String {
    state
        .sessions
        .create(Principal {
            role: PrincipalRole::Admin,
            display_name: "Admin".to_string(),
            legal_name: "Admin".to_string(),
            ref_: "admin".to_string(),
            phone: "+998880000000".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

fn request(method: &str, uri: &str, token: &str) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if !token.trim().is_empty() {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::empty()).expect("request")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

struct FakeStockLookup {
    not_found: bool,
}

impl FakeStockLookup {
    fn found() -> Self {
        Self { not_found: false }
    }

    fn not_found() -> Self {
        Self { not_found: true }
    }
}

#[async_trait]
impl WerkaHomeLookup for FakeStockLookup {
    async fn stock_entries_by_barcode(
        &self,
        barcode: &str,
        _limit: usize,
    ) -> Result<Vec<StockEntryBarcodeEntry>, WerkaPortError> {
        if self.not_found {
            return Err(WerkaPortError::NotFound);
        }
        Ok(vec![StockEntryBarcodeEntry {
            stock_entry_name: "MAT-STE-001".to_string(),
            stock_entry_type: "Material Receipt".to_string(),
            doc_status: 1,
            status: "Submitted".to_string(),
            company: "Company".to_string(),
            posting_date: "2026-01-26".to_string(),
            posting_time: "10:00:00".to_string(),
            creation: "2026-01-26 10:00:00".to_string(),
            modified: "2026-01-26 10:01:00".to_string(),
            remarks: "Remark".to_string(),
            line_index: 1,
            item_code: "ITEM-001".to_string(),
            item_name: "Rice".to_string(),
            qty: 2.0,
            uom: "Kg".to_string(),
            stock_uom: "Kg".to_string(),
            barcode: barcode.to_string(),
            source_warehouse: "Source - CH".to_string(),
            target_warehouse: "Stores - CH".to_string(),
        }])
    }
}
