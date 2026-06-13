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
use crate::core::werka::models::SupplierItem;
use crate::core::werka::ports::{SupplierItemLookup, WerkaPortError};
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
async fn supplier_items_accepts_post_and_filters_query_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_supplier_item_lookup(Arc::new(FakeSupplierItems));
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(request("POST", "/v1/mobile/supplier/items?q=milk", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value.as_array().expect("array").len(), 1);
    assert_eq!(value[0]["code"], "ITEM-MILK");
    assert_eq!(value[0]["warehouse"], "Stores - CH");
}

#[tokio::test]
async fn supplier_items_forbids_non_supplier_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_supplier_item_lookup(Arc::new(FakeSupplierItems));
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/supplier/items", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn supplier_items_fails_without_provider_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/supplier/items", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"], "supplier items failed");
}

fn request(method: &str, uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request")
}

async fn json_body(response: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("json")
}

async fn supplier_session(state: &AppState) -> String {
    state
        .sessions
        .create(Principal {
            role: PrincipalRole::Supplier,
            display_name: "Supplier".to_string(),
            legal_name: "Supplier".to_string(),
            ref_: "SUP-001".to_string(),
            phone: "+998901111111".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
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

struct FakeSupplierItems;

#[async_trait]
impl SupplierItemLookup for FakeSupplierItems {
    async fn list_assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        assert_eq!(supplier_ref, "SUP-001");
        assert_eq!(limit, 20);
        Ok(vec![
            supplier_item("ITEM-MILK", "Fresh Milk"),
            supplier_item("ITEM-BREAD", "Bread"),
        ])
    }

    async fn get_supplier_items_by_codes(
        &self,
        _item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        Ok(Vec::new())
    }
}

fn supplier_item(code: &str, name: &str) -> SupplierItem {
    SupplierItem {
        code: code.to_string(),
        name: name.to_string(),
        uom: "Nos".to_string(),
        warehouse: "Stores - CH".to_string(),
        item_group: String::new(),
    }
}
