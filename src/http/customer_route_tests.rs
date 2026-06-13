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
use crate::core::customer::ports::{
    CustomerDeliveryNoteDraft, CustomerDeliveryPort, CustomerPortError,
};
use crate::core::customer::service::CustomerService;
use crate::core::push::ports::{PushSendError, PushSenderPort};
use crate::core::push::service::PushService;
use crate::core::session::manager::SessionManager;
use crate::core::werka::ports::DeliveryNoteStateUpdate;

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
async fn customer_summary_accepts_post_like_go() {
    let mut state = test_state();
    state.customer = CustomerService::new().with_delivery_port(Arc::new(FakeDeliveryPort));
    let token = customer_session(&state).await;

    let response = build_router(state)
        .oneshot(request("POST", "/v1/mobile/customer/summary", &token, ""))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["pending_count"], 1);
    assert_eq!(value["confirmed_count"], 1);
    assert_eq!(value["rejected_count"], 1);
}

#[tokio::test]
async fn customer_history_forbids_non_customer_like_go() {
    let mut state = test_state();
    state.customer = CustomerService::new().with_delivery_port(Arc::new(FakeDeliveryPort));
    let token = session(&state, PrincipalRole::Werka).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/customer/history", &token, ""))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn customer_detail_requires_delivery_note_id_like_go() {
    let state = test_state();
    let token = customer_session(&state).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/customer/detail", &token, ""))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await["error"],
        "delivery_note_id is required"
    );
}

#[tokio::test]
async fn customer_respond_rejects_get_like_go() {
    let state = test_state();
    let token = customer_session(&state).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/customer/respond", &token, ""))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn customer_respond_returns_detail_like_go() {
    let mut state = test_state();
    state.customer = CustomerService::new().with_delivery_port(Arc::new(FakeDeliveryPort));
    let token = customer_session(&state).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/customer/respond",
            &token,
            r#"{"delivery_note_id":"DN-PENDING","approve":true}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["record"]["id"], "DN-PENDING");
    assert_eq!(value["record"]["status"], "accepted");
    assert_eq!(value["can_approve"], false);
}

#[tokio::test]
async fn customer_respond_sends_werka_and_admin_push_like_go() {
    let sender = Arc::new(RecordingPushSender::default());
    let mut state = test_state();
    state.customer = CustomerService::new().with_delivery_port(Arc::new(FakeDeliveryPort));
    state.push = PushService::new(state.push.store_for_tests()).with_sender(sender.clone());
    let token = customer_session(&state).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/customer/respond",
            &token,
            r#"{"delivery_note_id":"DN-PENDING","approve":true}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let calls = sender.calls.lock().expect("calls");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].key, "werka:werka");
    assert_eq!(calls[0].title, "Customer javob berdi");
    assert_eq!(calls[0].data["target_role"], "werka");
    assert_eq!(calls[1].key, "admin:admin");
    assert_eq!(calls[1].data["target_role"], "admin");
}

struct FakeDeliveryPort;

#[async_trait]
impl CustomerDeliveryPort for FakeDeliveryPort {
    async fn list_customer_delivery_notes_page(
        &self,
        _customer: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<CustomerDeliveryNoteDraft>, CustomerPortError> {
        Ok(vec![
            delivery("DN-PENDING", "1", "1"),
            delivery("DN-ACCEPTED", "1", "3"),
            delivery("DN-PARTIAL", "1", "4"),
        ])
    }

    async fn get_delivery_note(
        &self,
        name: &str,
    ) -> Result<CustomerDeliveryNoteDraft, CustomerPortError> {
        Ok(delivery(name, "1", "1"))
    }

    async fn create_and_submit_delivery_note_return(
        &self,
        _source_name: &str,
    ) -> Result<(), CustomerPortError> {
        Ok(())
    }

    async fn create_and_submit_partial_delivery_note_return(
        &self,
        _source_name: &str,
        _returned_qty: f64,
    ) -> Result<(), CustomerPortError> {
        Ok(())
    }

    async fn update_delivery_note_remarks(
        &self,
        _name: &str,
        _remarks: &str,
    ) -> Result<(), CustomerPortError> {
        Ok(())
    }

    async fn update_delivery_note_state(
        &self,
        _name: &str,
        _update: DeliveryNoteStateUpdate,
    ) -> Result<(), CustomerPortError> {
        Ok(())
    }
}

async fn customer_session(state: &AppState) -> String {
    session(state, PrincipalRole::Customer).await
}

async fn session(state: &AppState, role: PrincipalRole) -> String {
    state
        .sessions
        .create(Principal {
            role,
            display_name: "Customer".to_string(),
            legal_name: "Customer".to_string(),
            ref_: "CUST-001".to_string(),
            phone: "+998901234567".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

fn request(method: &str, uri: &str, token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from(body.to_string()))
        .expect("request")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

fn delivery(name: &str, flow_state: &str, customer_state: &str) -> CustomerDeliveryNoteDraft {
    CustomerDeliveryNoteDraft {
        name: name.to_string(),
        customer: "CUST-001".to_string(),
        customer_name: "Comfi".to_string(),
        posting_date: "2026-01-01".to_string(),
        modified: "2026-01-02 10:00:00".to_string(),
        doc_status: 1,
        accord_flow_state: flow_state.to_string(),
        accord_customer_state: customer_state.to_string(),
        item_code: "ITEM-001".to_string(),
        item_name: "Item".to_string(),
        qty: 5.0,
        uom: "Kg".to_string(),
        ..CustomerDeliveryNoteDraft::default()
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
