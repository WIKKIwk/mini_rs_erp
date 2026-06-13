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
use crate::core::werka::models::SupplierItem;
use crate::core::werka::ports::{
    CreatePurchaseReceiptInput, PurchaseReceiptDraft, SupplierItemLookup, WerkaPortError,
    WerkaSupplierRecord, WerkaUnannouncedWriter,
};
use crate::core::werka::service::WerkaService;

fn test_state() -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
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
async fn supplier_dispatch_rejects_non_post_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/supplier/dispatch", &token, ""))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn supplier_dispatch_forbids_non_supplier_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(dispatch_request(&token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn supplier_dispatch_rejects_invalid_json_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(request("POST", "/v1/mobile/supplier/dispatch", &token, "{"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid json");
}

#[tokio::test]
async fn supplier_dispatch_fails_without_provider_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(dispatch_request(&token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"], "dispatch create failed");
}

#[tokio::test]
async fn supplier_dispatch_returns_pending_record_and_forwards_input_like_go() {
    let writer = Arc::new(FakeDispatchWriter::default());
    let mut state = test_state();
    state.werka = WerkaService::new()
        .with_supplier_item_lookup(Arc::new(FakeSupplierItems))
        .with_unannounced_writer(writer.clone());
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(dispatch_request(&token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["id"], "PR-001");
    assert_eq!(value["supplier_name"], "Supplier");
    assert_eq!(value["item_code"], "ITEM-001");
    assert_eq!(value["item_name"], "Rice");
    assert_eq!(value["uom"], "Kg");
    assert_eq!(value["sent_qty"], 10.0);
    assert_eq!(value["accepted_qty"], 0.0);
    assert_eq!(value["status"], "pending");
    assert!(value.get("record_type").is_none());

    let input = writer
        .input
        .lock()
        .expect("input")
        .clone()
        .expect("purchase receipt input");
    assert_eq!(input.supplier, "SUP-001");
    assert_eq!(input.supplier_phone, "+998901111111");
    assert_eq!(input.item_code, "ITEM-001");
    assert_eq!(input.qty, 10.0);
    assert_eq!(input.warehouse, "Stores - CH");
}

#[tokio::test]
async fn supplier_dispatch_sends_werka_push_like_go() {
    let writer = Arc::new(FakeDispatchWriter::default());
    let sender = Arc::new(RecordingPushSender::default());
    let mut state = test_state();
    state.werka = WerkaService::new()
        .with_supplier_item_lookup(Arc::new(FakeSupplierItems))
        .with_unannounced_writer(writer);
    state.push = PushService::new(state.push.store_for_tests()).with_sender(sender.clone());
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(dispatch_request(&token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let calls = sender.calls.lock().expect("calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].key, "werka:werka");
    assert_eq!(calls[0].title, "Supplier");
    assert_eq!(calls[0].body, "ITEM-001 • 10 Kg qabul kutmoqda.");
    assert_eq!(calls[0].data["target_role"], "werka");
    assert_eq!(calls[0].data["sent_qty"], "10.0000");
}

fn dispatch_request(token: &str) -> Request<Body> {
    request(
        "POST",
        "/v1/mobile/supplier/dispatch",
        token,
        r#"{"item_code":" ITEM-001 ","qty":10}"#,
    )
}

fn request(method: &str, uri: &str, token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
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
        assert_eq!(limit, 200);
        Ok(vec![SupplierItem {
            code: "ITEM-001".to_string(),
            name: "Rice".to_string(),
            uom: "Kg".to_string(),
            warehouse: "Stores - CH".to_string(),
            item_group: String::new(),
        }])
    }

    async fn get_supplier_items_by_codes(
        &self,
        _item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, WerkaPortError> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct FakeDispatchWriter {
    input: Mutex<Option<CreatePurchaseReceiptInput>>,
}

#[async_trait]
impl WerkaUnannouncedWriter for FakeDispatchWriter {
    async fn find_supplier_for_werka(
        &self,
        supplier_ref: &str,
    ) -> Result<WerkaSupplierRecord, WerkaPortError> {
        Ok(WerkaSupplierRecord {
            id: supplier_ref.to_string(),
            name: "Supplier".to_string(),
            phone: "+998901111111".to_string(),
        })
    }

    async fn validate_supplier_item_allowed(
        &self,
        _supplier_ref: &str,
        _item_code: &str,
    ) -> Result<(), WerkaPortError> {
        Ok(())
    }

    async fn resolve_warehouse(&self) -> Result<String, WerkaPortError> {
        Ok("Stores - CH".to_string())
    }

    async fn create_draft_purchase_receipt(
        &self,
        input: CreatePurchaseReceiptInput,
    ) -> Result<PurchaseReceiptDraft, WerkaPortError> {
        *self.input.lock().expect("input") = Some(input);
        Ok(PurchaseReceiptDraft {
            name: "PR-001".to_string(),
            doc_status: 0,
            status: "Draft".to_string(),
            supplier: "SUP-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Rice".to_string(),
            qty: 10.0,
            uom: "Kg".to_string(),
            ..PurchaseReceiptDraft::default()
        })
    }

    async fn update_purchase_receipt_remarks(
        &self,
        _name: &str,
        _remarks: &str,
    ) -> Result<(), WerkaPortError> {
        Ok(())
    }

    async fn add_purchase_receipt_comment(
        &self,
        _name: &str,
        _content: &str,
    ) -> Result<(), WerkaPortError> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordingPushSender {
    calls: Mutex<Vec<PushCall>>,
}

#[derive(Clone)]
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
