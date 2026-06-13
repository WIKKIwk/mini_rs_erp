use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::push::ports::{PushSendError, PushSenderPort};
use crate::core::push::service::PushService;
use crate::core::session::manager::SessionManager;
use crate::core::werka::ports::{
    CreatePurchaseReceiptInput, PurchaseReceiptDraft, WerkaPortError, WerkaSupplierRecord,
    WerkaUnannouncedWriter,
};
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
async fn unannounced_create_rejects_non_post_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/werka/unannounced/create")
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
async fn unannounced_create_rejects_invalid_json_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(create_request(&token, "{"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid json");
}

#[tokio::test]
async fn unannounced_create_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(create_request(&token, request_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "werka unannounced create failed"
    );
}

#[tokio::test]
async fn unannounced_create_returns_dispatch_record_and_marks_pending() {
    let mut state = test_state();
    state.werka = WerkaService::new().with_unannounced_writer(Arc::new(FakeUnannouncedWriter));
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(create_request(&token, request_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["id"], "PR-001");
    assert_eq!(value["record_type"], "purchase_receipt");
    assert_eq!(value["supplier_ref"], "SUP-001");
    assert_eq!(value["event_type"], "werka_unannounced_pending");
    assert_eq!(
        value["highlight"],
        "Werka siz qayd etmagan mahsulotni qabul qildi"
    );
}

#[tokio::test]
async fn unannounced_create_sends_supplier_push_like_go() {
    let sender = Arc::new(RecordingPushSender::default());
    let mut state = test_state();
    state.werka = WerkaService::new().with_unannounced_writer(Arc::new(FakeUnannouncedWriter));
    state.push = PushService::new(state.push.store_for_tests()).with_sender(sender.clone());
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(create_request(&token, request_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let calls = sender.calls.lock().expect("calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].key, "supplier:SUP-001");
    assert_eq!(
        calls[0].title,
        "Werka siz qayd etmagan mahsulotni qabul qildi"
    );
    assert_eq!(calls[0].body, "Tasdiqlash kutilmoqda");
    assert_eq!(calls[0].data["target_role"], "supplier");
    assert_eq!(calls[0].data["target_ref"], "SUP-001");
    assert_eq!(calls[0].data["event_type"], "werka_unannounced_pending");
}

fn request_body() -> &'static str {
    r#"{"supplier_ref":"SUP-001","item_code":"ITEM-001","qty":2}"#
}

fn create_request(token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/mobile/werka/unannounced/create")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
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

struct FakeUnannouncedWriter;

#[async_trait]
impl WerkaUnannouncedWriter for FakeUnannouncedWriter {
    async fn find_supplier_for_werka(
        &self,
        supplier_ref: &str,
    ) -> Result<WerkaSupplierRecord, WerkaPortError> {
        assert_eq!(supplier_ref, "SUP-001");
        Ok(WerkaSupplierRecord {
            id: "SUP-001".to_string(),
            name: "Supplier".to_string(),
            phone: "+998901111111".to_string(),
        })
    }

    async fn validate_supplier_item_allowed(
        &self,
        supplier_ref: &str,
        item_code: &str,
    ) -> Result<(), WerkaPortError> {
        assert_eq!(supplier_ref, "SUP-001");
        assert_eq!(item_code, "ITEM-001");
        Ok(())
    }

    async fn resolve_warehouse(&self) -> Result<String, WerkaPortError> {
        Ok("Stores - A".to_string())
    }

    async fn create_draft_purchase_receipt(
        &self,
        input: CreatePurchaseReceiptInput,
    ) -> Result<PurchaseReceiptDraft, WerkaPortError> {
        assert_eq!(input.supplier, "SUP-001");
        assert_eq!(input.item_code, "ITEM-001");
        assert_eq!(input.qty, 2.0);
        Ok(PurchaseReceiptDraft {
            name: "PR-001".to_string(),
            doc_status: 0,
            status: "Draft".to_string(),
            supplier: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            posting_date: "2026-01-16".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item 001".to_string(),
            qty: 2.0,
            uom: "Kg".to_string(),
            ..PurchaseReceiptDraft::default()
        })
    }

    async fn update_purchase_receipt_remarks(
        &self,
        name: &str,
        remarks: &str,
    ) -> Result<(), WerkaPortError> {
        assert_eq!(name, "PR-001");
        assert!(remarks.contains("Accord Werka Aytilmagan: pending"));
        Ok(())
    }

    async fn add_purchase_receipt_comment(
        &self,
        name: &str,
        content: &str,
    ) -> Result<(), WerkaPortError> {
        assert_eq!(name, "PR-001");
        assert!(content.contains("Aytilmagan mol sifatida qayd qilindi."));
        Ok(())
    }
}

#[derive(Default)]
struct RecordingPushSender {
    calls: Mutex<Vec<PushCall>>,
}

#[derive(Debug)]
struct PushCall {
    key: String,
    title: String,
    body: String,
    data: HashMap<String, String>,
}

#[async_trait]
impl PushSenderPort for RecordingPushSender {
    async fn send_to_key(
        &self,
        key: &str,
        title: &str,
        body: &str,
        data: HashMap<String, String>,
    ) -> Result<(), PushSendError> {
        self.calls.lock().expect("calls").push(PushCall {
            key: key.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            data,
        });
        Ok(())
    }
}
