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
    PurchaseReceiptComment, PurchaseReceiptDraft, PurchaseReceiptSubmissionResult,
    SupplierUnannouncedWriter, WerkaPortError,
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
async fn supplier_unannounced_respond_rejects_non_post_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/supplier/unannounced/respond")
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
async fn supplier_unannounced_respond_forbids_non_supplier_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(create_request(&token, approve_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn supplier_unannounced_respond_rejects_invalid_json_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;
    let response = build_router(state)
        .oneshot(create_request(&token, "{"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid json");
}

#[tokio::test]
async fn supplier_unannounced_respond_fails_without_provider_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;
    let response = build_router(state)
        .oneshot(create_request(&token, approve_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "supplier unannounced response failed"
    );
}

#[tokio::test]
async fn supplier_unannounced_respond_approves_pending_receipt() {
    let mut state = test_state();
    state.werka =
        WerkaService::new().with_supplier_unannounced_writer(Arc::new(FakeSupplierWriter::new()));
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(create_request(&token, approve_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["record"]["id"], "PR-001");
    assert_eq!(value["record"]["status"], "accepted");
    assert_eq!(value["record"]["accepted_qty"], 2.0);
    assert!(value["record"].get("event_type").is_none());
    assert!(value["record"].get("highlight").is_none());
    assert!(value["record"].get("note").is_none());
}

#[tokio::test]
async fn supplier_unannounced_respond_sends_werka_push_like_go() {
    let sender = Arc::new(RecordingPushSender::default());
    let mut state = test_state();
    state.werka =
        WerkaService::new().with_supplier_unannounced_writer(Arc::new(FakeSupplierWriter::new()));
    state.push = PushService::new(state.push.store_for_tests()).with_sender(sender.clone());
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(create_request(&token, approve_body()))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let calls = sender.calls.lock().expect("calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].key, "werka:werka");
    assert_eq!(calls[0].title, "Supplier javob berdi");
    assert_eq!(calls[0].data["target_role"], "werka");
    assert_eq!(calls[0].data["target_ref"], "werka");
    assert_eq!(calls[0].data["id"], "PR-001");
}

#[tokio::test]
async fn supplier_unannounced_respond_rejects_pending_receipt_with_reason() {
    let mut state = test_state();
    state.werka =
        WerkaService::new().with_supplier_unannounced_writer(Arc::new(FakeSupplierWriter::new()));
    let token = supplier_session(&state).await;

    let response = build_router(state)
        .oneshot(create_request(
            &token,
            r#"{"receipt_id":"PR-001","approve":false,"reason":"Ortiqcha"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["record"]["status"], "cancelled");
    assert_eq!(
        value["record"]["note"],
        "Supplier aytilmagan molni rad etdi.\nSabab: Ortiqcha"
    );
    assert_eq!(value["comments"][0]["author_label"], "Supplier • Supplier");
}

fn approve_body() -> &'static str {
    r#"{"receipt_id":"PR-001","approve":true,"reason":""}"#
}

fn create_request(token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/mobile/supplier/unannounced/respond")
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

struct FakeSupplierWriter {
    remarks: Mutex<String>,
    comments: Mutex<Vec<PurchaseReceiptComment>>,
}

impl FakeSupplierWriter {
    fn new() -> Self {
        Self {
            remarks: Mutex::new("Accord Werka Aytilmagan: pending".to_string()),
            comments: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl SupplierUnannouncedWriter for FakeSupplierWriter {
    async fn get_purchase_receipt(
        &self,
        name: &str,
    ) -> Result<PurchaseReceiptDraft, WerkaPortError> {
        assert_eq!(name, "PR-001");
        let remarks = self.remarks.lock().expect("remarks").clone();
        let state = if remarks.contains("approved") { 1 } else { 0 };
        let status = if state == 1 { "Submitted" } else { "Draft" };
        Ok(PurchaseReceiptDraft {
            name: "PR-001".to_string(),
            doc_status: state,
            status: status.to_string(),
            supplier: "SUP-001".to_string(),
            supplier_name: "Supplier Legal".to_string(),
            posting_date: "2026-01-16".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item 001".to_string(),
            qty: 2.0,
            uom: "Kg".to_string(),
            remarks,
            ..PurchaseReceiptDraft::default()
        })
    }

    async fn update_purchase_receipt_remarks(
        &self,
        name: &str,
        remarks: &str,
    ) -> Result<(), WerkaPortError> {
        assert_eq!(name, "PR-001");
        *self.remarks.lock().expect("remarks") = remarks.to_string();
        Ok(())
    }

    async fn confirm_and_submit_purchase_receipt(
        &self,
        name: &str,
        accepted_qty: f64,
        returned_qty: f64,
        return_reason: &str,
        return_comment: &str,
    ) -> Result<PurchaseReceiptSubmissionResult, WerkaPortError> {
        assert_eq!(name, "PR-001");
        assert_eq!(accepted_qty, 2.0);
        assert_eq!(returned_qty, 0.0);
        assert!(return_reason.is_empty());
        assert!(return_comment.is_empty());
        Ok(PurchaseReceiptSubmissionResult {
            name: "PR-001".to_string(),
            supplier: "SUP-001".to_string(),
            item_code: "ITEM-001".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 2.0,
            accepted_qty: 2.0,
            ..PurchaseReceiptSubmissionResult::default()
        })
    }

    async fn add_purchase_receipt_comment(
        &self,
        name: &str,
        content: &str,
    ) -> Result<(), WerkaPortError> {
        assert_eq!(name, "PR-001");
        self.comments
            .lock()
            .expect("comments")
            .push(PurchaseReceiptComment {
                id: format!("COMMENT-{}", content.len()),
                content: content.to_string(),
                created_at: "2026-01-16 10:00:00".to_string(),
            });
        Ok(())
    }

    async fn list_purchase_receipt_comments(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<Vec<PurchaseReceiptComment>, WerkaPortError> {
        assert_eq!(name, "PR-001");
        assert_eq!(limit, 100);
        Ok(self.comments.lock().expect("comments").clone())
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
    data: HashMap<String, String>,
}

#[async_trait]
impl PushSenderPort for RecordingPushSender {
    async fn send_to_key(
        &self,
        key: &str,
        title: &str,
        _body: &str,
        data: HashMap<String, String>,
    ) -> Result<(), PushSendError> {
        self.calls.lock().expect("calls").push(PushCall {
            key: key.to_string(),
            title: title.to_string(),
            data,
        });
        Ok(())
    }
}
