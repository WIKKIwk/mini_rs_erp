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
use crate::core::werka::ports::{
    DeliveryNoteNotificationDraft, NotificationDetailWriter, PurchaseReceiptComment,
    PurchaseReceiptDraft, WerkaPortError,
};
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
async fn notification_detail_rejects_non_get_like_go() {
    let state = test_state();
    let token = supplier_session(&state, "SUP-001").await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/notifications/detail?receipt_id=PR-001")
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
async fn notification_detail_requires_receipt_id_like_go() {
    let state = test_state();
    let token = supplier_session(&state, "SUP-001").await;
    let response = build_router(state)
        .oneshot(get_request(&token, "/v1/mobile/notifications/detail"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "receipt_id is required");
}

#[tokio::test]
async fn notification_detail_forbids_admin_like_go() {
    let state = test_state();
    let token = admin_session(&state).await;
    let response = build_router(state)
        .oneshot(get_request(
            &token,
            "/v1/mobile/notifications/detail?receipt_id=PR-001",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn notification_detail_fails_without_provider_like_go() {
    let state = test_state();
    let token = supplier_session(&state, "SUP-001").await;
    let response = build_router(state)
        .oneshot(get_request(
            &token,
            "/v1/mobile/notifications/detail?receipt_id=PR-001",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "notification detail failed"
    );
}

#[tokio::test]
async fn notification_detail_returns_purchase_receipt_for_supplier() {
    let mut state = test_state();
    state.werka =
        WerkaService::new().with_notification_detail_writer(Arc::new(FakeNotificationWriter));
    let token = supplier_session(&state, "SUP-001").await;
    let response = build_router(state)
        .oneshot(get_request(
            &token,
            "/v1/mobile/notifications/detail?receipt_id=PR-001",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["record"]["id"], "PR-001");
    assert_eq!(value["record"]["supplier_name"], "Supplier");
    assert_eq!(value["record"]["event_type"], "werka_unannounced_pending");
    assert_eq!(value["comments"][0]["author_label"], "Werka");
}

#[tokio::test]
async fn notification_detail_forbids_customer_purchase_receipt_like_go() {
    let mut state = test_state();
    state.werka =
        WerkaService::new().with_notification_detail_writer(Arc::new(FakeNotificationWriter));
    let token = customer_session(&state, "CUST-001").await;
    let response = build_router(state)
        .oneshot(get_request(
            &token,
            "/v1/mobile/notifications/detail?receipt_id=PR-001",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn notification_detail_returns_customer_delivery_result_event() {
    let mut state = test_state();
    state.werka =
        WerkaService::new().with_notification_detail_writer(Arc::new(FakeNotificationWriter));
    let token = customer_session(&state, "CUST-001").await;
    let response = build_router(state)
        .oneshot(get_request(
            &token,
            "/v1/mobile/notifications/detail?receipt_id=customer_delivery_result:DN-001",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["record"]["id"], "customer_delivery_result:DN-001");
    assert_eq!(value["record"]["record_type"], "delivery_note");
    assert_eq!(value["record"]["event_type"], "customer_delivery_partial");
    assert_eq!(value["record"]["status"], "partial");
}

fn get_request(token: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
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

async fn supplier_session(state: &AppState, ref_: &str) -> String {
    create_session(state, PrincipalRole::Supplier, ref_).await
}

async fn customer_session(state: &AppState, ref_: &str) -> String {
    create_session(state, PrincipalRole::Customer, ref_).await
}

async fn admin_session(state: &AppState) -> String {
    create_session(state, PrincipalRole::Admin, "admin").await
}

async fn create_session(state: &AppState, role: PrincipalRole, ref_: &str) -> String {
    state
        .sessions
        .create(Principal {
            role,
            display_name: "Supplier".to_string(),
            legal_name: "Supplier".to_string(),
            ref_: ref_.to_string(),
            phone: "+998901111111".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

struct FakeNotificationWriter;

#[async_trait]
impl NotificationDetailWriter for FakeNotificationWriter {
    async fn get_notification_purchase_receipt(
        &self,
        name: &str,
    ) -> Result<PurchaseReceiptDraft, WerkaPortError> {
        assert_eq!(name, "PR-001");
        Ok(PurchaseReceiptDraft {
            name: "PR-001".to_string(),
            doc_status: 0,
            status: "Draft".to_string(),
            supplier: "SUP-001".to_string(),
            supplier_name: "Supplier Legal".to_string(),
            posting_date: "2026-01-16".to_string(),
            supplier_delivery_note: "TG:+998901111111:20260116100000:2.0000".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item 001".to_string(),
            qty: 2.0,
            uom: "Kg".to_string(),
            remarks: "Accord Werka Aytilmagan: pending".to_string(),
            ..PurchaseReceiptDraft::default()
        })
    }

    async fn list_notification_purchase_receipt_comments(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<Vec<PurchaseReceiptComment>, WerkaPortError> {
        assert_eq!(name, "PR-001");
        assert_eq!(limit, 100);
        Ok(vec![PurchaseReceiptComment {
            id: "C-001".to_string(),
            content: "Werka\nAytilmagan mol sifatida qayd qilindi.".to_string(),
            created_at: "2026-01-16 10:00:00".to_string(),
        }])
    }

    async fn get_notification_delivery_note(
        &self,
        name: &str,
    ) -> Result<DeliveryNoteNotificationDraft, WerkaPortError> {
        assert_eq!(name, "DN-001");
        Ok(DeliveryNoteNotificationDraft {
            name: "DN-001".to_string(),
            customer: "CUST-001".to_string(),
            customer_name: "Customer".to_string(),
            doc_status: 1,
            modified: "2026-01-16 10:00:00".to_string(),
            qty: 10.0,
            returned_qty: 3.0,
            accord_customer_reason: "Siniq".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item 001".to_string(),
            uom: "Kg".to_string(),
            accord_flow_state: 1,
            accord_customer_state: 4,
            ..DeliveryNoteNotificationDraft::default()
        })
    }

    async fn list_notification_delivery_note_comments(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<Vec<PurchaseReceiptComment>, WerkaPortError> {
        assert_eq!(name, "DN-001");
        assert_eq!(limit, 100);
        Ok(Vec::new())
    }
}
