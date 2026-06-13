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
use crate::core::werka::models::{CustomerItemOption, SupplierItem};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::core::werka::service::WerkaService;

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
    });
    state.sessions = SessionManager::memory(Some(30 * 24 * 60 * 60));
    state
}

#[tokio::test]
async fn werka_item_endpoints_require_auth() {
    for uri in [
        "/v1/mobile/werka/supplier-items",
        "/v1/mobile/werka/customer-items",
        "/v1/mobile/werka/customer-item-options",
    ] {
        let response = build_router(test_state())
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn werka_item_endpoints_fail_without_provider_like_go() {
    for (uri, error) in [
        (
            "/v1/mobile/werka/supplier-items",
            "werka supplier items failed",
        ),
        (
            "/v1/mobile/werka/customer-items",
            "werka customer items failed",
        ),
        (
            "/v1/mobile/werka/customer-item-options",
            "werka customer item options failed",
        ),
    ] {
        let state = test_state();
        let token = werka_session(&state).await;
        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(json_body(response).await["error"], error);
    }
}

#[tokio::test]
async fn werka_supplier_items_returns_provider_payload_and_parses_query() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeItemsLookup));
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/supplier-items?supplier_ref=%20SUP-001%20&q=%20milk%20&limit=999&offset=3")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value[0]["code"], "ITEM-001");
    assert_eq!(value[0]["warehouse"], "Stores - A");
}

#[tokio::test]
async fn werka_customer_items_returns_provider_payload_and_parses_query() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeItemsLookup));
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/customer-items?customer_ref=%20CUST-001%20&q=%20milk%20&limit=999&offset=3")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value[0]["code"], "ITEM-002");
    assert_eq!(value[0]["name"], "Customer Milk");
}

#[tokio::test]
async fn werka_customer_item_options_returns_provider_payload_and_parses_query() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_lookup(Arc::new(FakeItemsLookup));
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .uri("/v1/mobile/werka/customer-item-options?q=%20milk%20&limit=999&offset=3")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value[0]["customer_ref"], "CUST-001");
    assert_eq!(value[0]["item_code"], "ITEM-003");
}

#[tokio::test]
async fn werka_item_endpoints_default_invalid_limit_and_offset_like_go() {
    for uri in [
        "/v1/mobile/werka/supplier-items?limit=abc&offset=-9",
        "/v1/mobile/werka/customer-items?limit=abc&offset=-9",
        "/v1/mobile/werka/customer-item-options?limit=abc&offset=-9",
    ] {
        let mut state = test_state();
        state.werka = WerkaService::new().with_lookup(Arc::new(DefaultItemsLookup));
        let token = werka_session(&state).await;
        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn werka_item_endpoints_accept_post_like_go_handler() {
    for uri in [
        "/v1/mobile/werka/supplier-items?supplier_ref=SUP-001&q=milk",
        "/v1/mobile/werka/customer-items?customer_ref=CUST-001&q=milk",
        "/v1/mobile/werka/customer-item-options?q=milk",
    ] {
        let mut state = test_state();
        state.werka = WerkaService::new().with_lookup(Arc::new(PostItemsLookup));
        let token = werka_session(&state).await;
        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }
}

async fn json_body(response: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("json")
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

struct FakeItemsLookup;
struct DefaultItemsLookup;
struct PostItemsLookup;

#[async_trait]
impl WerkaHomeLookup for FakeItemsLookup {
    async fn werka_supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        assert_eq!(supplier_ref, "SUP-001");
        assert_eq!(query, "milk");
        assert_eq!(limit, 200);
        assert_eq!(offset, 3);
        Ok(vec![supplier_item("ITEM-001", "Supplier Milk")])
    }

    async fn werka_customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        assert_eq!(customer_ref, "CUST-001");
        assert_eq!(query, "milk");
        assert_eq!(limit, 200);
        assert_eq!(offset, 3);
        Ok(vec![supplier_item("ITEM-002", "Customer Milk")])
    }

    async fn werka_customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerItemOption>, WerkaPortError> {
        assert_eq!(query, "milk");
        assert_eq!(limit, 200);
        assert_eq!(offset, 3);
        Ok(vec![customer_option()])
    }
}

#[async_trait]
impl WerkaHomeLookup for DefaultItemsLookup {
    async fn werka_supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        assert_eq!(supplier_ref, "");
        assert_eq!(query, "");
        assert_eq!(limit, 100);
        assert_eq!(offset, 0);
        Ok(Vec::new())
    }

    async fn werka_customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        assert_eq!(customer_ref, "");
        assert_eq!(query, "");
        assert_eq!(limit, 100);
        assert_eq!(offset, 0);
        Ok(Vec::new())
    }

    async fn werka_customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerItemOption>, WerkaPortError> {
        assert_eq!(query, "");
        assert_eq!(limit, 200);
        assert_eq!(offset, 0);
        Ok(Vec::new())
    }
}

#[async_trait]
impl WerkaHomeLookup for PostItemsLookup {}

fn supplier_item(code: &str, name: &str) -> SupplierItem {
    SupplierItem {
        code: code.to_string(),
        name: name.to_string(),
        uom: "Kg".to_string(),
        warehouse: "Stores - A".to_string(),
        item_group: String::new(),
    }
}

fn customer_option() -> CustomerItemOption {
    CustomerItemOption {
        customer_ref: "CUST-001".to_string(),
        customer_name: "Ali Market".to_string(),
        customer_phone: "+998901111111".to_string(),
        item_code: "ITEM-003".to_string(),
        item_name: "Option Milk".to_string(),
        uom: "Kg".to_string(),
        warehouse: "Stores - A".to_string(),
    }
}
