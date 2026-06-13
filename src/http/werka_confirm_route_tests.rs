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
    PurchaseReceiptSubmissionResult, WerkaConfirmWriter, WerkaPortError,
};
use crate::core::werka::service::WerkaService;

#[tokio::test]
async fn werka_confirm_rejects_non_post_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/mobile/werka/confirm")
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
async fn werka_confirm_forbids_non_werka_like_go() {
    let state = test_state();
    let token = supplier_session(&state).await;
    let response = build_router(state)
        .oneshot(post_request(&token, r#"{"receipt_id":"PR-001"}"#))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn werka_confirm_rejects_invalid_json_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(post_request(&token, "{"))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid json");
}

#[tokio::test]
async fn werka_confirm_fails_without_provider_like_go() {
    let state = test_state();
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(post_request(&token, r#"{"receipt_id":"PR-001"}"#))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json_body(response).await["error"], "receipt confirm failed");
}

#[tokio::test]
async fn werka_confirm_returns_dispatch_record_and_passes_decision_fields() {
    let mut state = test_state();
    let writer = Arc::new(FakeConfirmWriter::default());
    state.werka = WerkaService::new().with_confirm_writer(writer.clone());
    let token = werka_session(&state).await;
    let response = build_router(state)
        .oneshot(post_request(
            &token,
            r#"{"receipt_id":" PR-001 ","accepted_qty":7,"returned_qty":3,"return_reason":"Brak","return_comment":"Qop yorilgan"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["id"], "PR-001");
    assert_eq!(value["supplier_name"], "SUP-001");
    assert_eq!(value["status"], "partial");
    assert_eq!(value["note"], "Qaytarildi");
    assert!(value.get("supplier_ref").is_none());
    assert_eq!(
        writer.calls.lock().expect("calls")[0],
        ConfirmCall {
            name: "PR-001".to_string(),
            accepted_qty: 7.0,
            returned_qty: 3.0,
            return_reason: "Brak".to_string(),
            return_comment: "Qop yorilgan".to_string(),
        }
    );
}

#[tokio::test]
async fn werka_confirm_sends_supplier_push_like_go() {
    let sender = Arc::new(RecordingPushSender::default());
    let mut state = test_state();
    state.werka = WerkaService::new().with_confirm_writer(Arc::new(FakeConfirmWriter::default()));
    state.push = PushService::new(state.push.store_for_tests()).with_sender(sender.clone());
    let token = werka_session(&state).await;

    let response = build_router(state)
        .oneshot(post_request(
            &token,
            r#"{"receipt_id":"PR-001","accepted_qty":7,"returned_qty":3,"return_reason":"Brak","return_comment":"Qop yorilgan"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let calls = sender.calls.lock().expect("calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].key, "supplier:");
    assert_eq!(calls[0].title, "ITEM-001");
    assert_eq!(calls[0].body, "Status: partial");
    assert_eq!(calls[0].data["target_role"], "supplier");
    assert_eq!(calls[0].data["target_ref"], "");
    assert_eq!(calls[0].data["sent_qty"], "10.0000");
}

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

async fn werka_session(state: &AppState) -> String {
    session(state, PrincipalRole::Werka, "werka").await
}

async fn supplier_session(state: &AppState) -> String {
    session(state, PrincipalRole::Supplier, "SUP-001").await
}

async fn session(state: &AppState, role: PrincipalRole, ref_: &str) -> String {
    state
        .sessions
        .create(Principal {
            role,
            display_name: "Werka".to_string(),
            legal_name: "Werka".to_string(),
            ref_: ref_.to_string(),
            phone: "+998901111111".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

fn post_request(token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/mobile/werka/confirm")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .expect("request")
}

async fn json_body(response: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("json")
}

#[derive(Debug, Clone, PartialEq)]
struct ConfirmCall {
    name: String,
    accepted_qty: f64,
    returned_qty: f64,
    return_reason: String,
    return_comment: String,
}

#[derive(Default)]
struct FakeConfirmWriter {
    calls: Mutex<Vec<ConfirmCall>>,
}

#[async_trait]
impl WerkaConfirmWriter for FakeConfirmWriter {
    async fn confirm_and_submit_purchase_receipt(
        &self,
        name: &str,
        accepted_qty: f64,
        returned_qty: f64,
        return_reason: &str,
        return_comment: &str,
    ) -> Result<PurchaseReceiptSubmissionResult, WerkaPortError> {
        self.calls.lock().expect("calls").push(ConfirmCall {
            name: name.to_string(),
            accepted_qty,
            returned_qty,
            return_reason: return_reason.to_string(),
            return_comment: return_comment.to_string(),
        });
        Ok(PurchaseReceiptSubmissionResult {
            name: "PR-001".to_string(),
            supplier: "SUP-001".to_string(),
            item_code: "ITEM-001".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty,
            supplier_delivery_note: "TG:+998901111111:20260116100000:10.0000".to_string(),
            note: "Qaytarildi".to_string(),
        })
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
