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
    DeliveryNoteNotificationDraft, NotificationDetailWriter, PurchaseReceiptComment,
    PurchaseReceiptDraft, WerkaPortError,
};
use crate::core::werka::service::WerkaService;

#[tokio::test]
async fn notification_comment_rejects_non_post_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;
    let response = build_router(state)
        .oneshot(get_request(
            &token,
            "/v1/mobile/notifications/comments?receipt_id=PR-001",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(json_body(response).await["error"], "method not allowed");
}

#[tokio::test]
async fn notification_comment_requires_receipt_id_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;
    let response = build_router(state)
        .oneshot(post_request(
            &token,
            "/v1/mobile/notifications/comments",
            "{}",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "receipt_id is required");
}

#[tokio::test]
async fn notification_comment_rejects_invalid_json_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;
    let response = build_router(state)
        .oneshot(post_request(
            &token,
            "/v1/mobile/notifications/comments?receipt_id=PR-001",
            "{",
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "invalid json");
}

#[tokio::test]
async fn notification_comment_forbids_admin_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin, "admin").await;
    let response = build_router(state)
        .oneshot(post_request(
            &token,
            "/v1/mobile/notifications/comments?receipt_id=PR-001",
            r#"{"message":"hello"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn notification_comment_empty_message_is_500_like_go() {
    let mut state = test_state();
    state.werka = WerkaService::new()
        .with_notification_detail_writer(Arc::new(RecordingNotificationWriter::default()));
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;
    let response = build_router(state)
        .oneshot(post_request(
            &token,
            "/v1/mobile/notifications/comments?receipt_id=PR-001",
            r#"{"message":"  "}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "notification comment failed"
    );
}

#[tokio::test]
async fn notification_comment_adds_purchase_receipt_comment() {
    let mut state = test_state();
    let writer = Arc::new(RecordingNotificationWriter::default());
    state.werka = WerkaService::new().with_notification_detail_writer(writer.clone());
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;
    let response = build_router(state)
        .oneshot(post_request(
            &token,
            "/v1/mobile/notifications/comments?receipt_id=PR-001",
            r#"{"message":"Qabul qilindi"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["comments"][1]["author_label"], "Supplier • Supplier");
    assert_eq!(value["comments"][1]["body"], "Qabul qilindi");
    assert_eq!(
        writer.purchase_comments.lock().expect("comments")[0],
        "Supplier • Supplier\nQabul qilindi"
    );
}

#[tokio::test]
async fn notification_comment_supplier_ack_updates_remarks_best_effort_like_go() {
    let mut state = test_state();
    let writer = Arc::new(RecordingNotificationWriter::default());
    state.werka = WerkaService::new().with_notification_detail_writer(writer.clone());
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;
    let response = build_router(state)
        .oneshot(post_request(
            &token,
            "/v1/mobile/notifications/comments?receipt_id=PR-001",
            r#"{"message":" tasdiqlayman, qaytdi "}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        writer.remarks.lock().expect("remarks")[0],
        "Old note\nAccord Supplier Tasdiq: tasdiqlayman, qaytdi"
    );
}

#[tokio::test]
async fn notification_comment_supplier_ack_sends_werka_push_like_go() {
    let sender = Arc::new(RecordingPushSender::default());
    let mut state = test_state();
    let writer = Arc::new(RecordingNotificationWriter::default());
    state.werka = WerkaService::new().with_notification_detail_writer(writer);
    state.push = PushService::new(state.push.store_for_tests()).with_sender(sender.clone());
    let token = session(&state, PrincipalRole::Supplier, "SUP-001").await;

    let response = build_router(state)
        .oneshot(post_request(
            &token,
            "/v1/mobile/notifications/comments?receipt_id=PR-001",
            r#"{"message":"tasdiqlayman"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let calls = sender.calls.lock().expect("calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].key, "werka:werka");
    assert_eq!(calls[0].title, "Supplier tasdiqladi");
    assert_eq!(
        calls[0].body,
        "Supplier mahsulotni qaytarganingizni tasdiqladi"
    );
    assert_eq!(calls[0].data["event_type"], "supplier_ack");
    assert_eq!(calls[0].data["target_role"], "werka");
    assert_eq!(calls[0].data["target_ref"], "werka");
}

#[tokio::test]
async fn notification_comment_adds_delivery_note_comment_for_customer() {
    let mut state = test_state();
    let writer = Arc::new(RecordingNotificationWriter::default());
    state.werka = WerkaService::new().with_notification_detail_writer(writer.clone());
    let token = session(&state, PrincipalRole::Customer, "CUST-001").await;
    let response = build_router(state)
        .oneshot(post_request(
            &token,
            "/v1/mobile/notifications/comments?receipt_id=customer_delivery_result:DN-001",
            r#"{"message":"Qisman qabul qildim"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        writer.delivery_comments.lock().expect("comments")[0],
        "Customer • Supplier\nQisman qabul qildim"
    );
}

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

fn get_request(token: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request")
}

fn post_request(token: &str, uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
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

async fn session(state: &AppState, role: PrincipalRole, ref_: &str) -> String {
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

#[derive(Default)]
struct RecordingNotificationWriter {
    purchase_comments: Mutex<Vec<String>>,
    delivery_comments: Mutex<Vec<String>>,
    remarks: Mutex<Vec<String>>,
}

#[async_trait]
impl NotificationDetailWriter for RecordingNotificationWriter {
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
            remarks: "Old note\nSupplier tasdiqladi: old".to_string(),
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
        let mut comments = vec![PurchaseReceiptComment {
            id: "C-001".to_string(),
            content: "Werka\nAytilmagan mol sifatida qayd qilindi.".to_string(),
            created_at: "2026-01-16 10:00:00".to_string(),
        }];
        comments.extend(
            self.purchase_comments
                .lock()
                .expect("comments")
                .iter()
                .enumerate()
                .map(|(index, content)| PurchaseReceiptComment {
                    id: format!("C-{}", index + 2),
                    content: content.clone(),
                    created_at: "2026-01-16 10:01:00".to_string(),
                }),
        );
        Ok(comments)
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
        Ok(self
            .delivery_comments
            .lock()
            .expect("comments")
            .iter()
            .enumerate()
            .map(|(index, content)| PurchaseReceiptComment {
                id: format!("DN-C-{}", index + 1),
                content: content.clone(),
                created_at: "2026-01-16 10:01:00".to_string(),
            })
            .collect())
    }

    async fn add_notification_purchase_receipt_comment(
        &self,
        name: &str,
        content: &str,
    ) -> Result<(), WerkaPortError> {
        assert_eq!(name, "PR-001");
        self.purchase_comments
            .lock()
            .expect("comments")
            .push(content.to_string());
        Ok(())
    }

    async fn update_notification_purchase_receipt_remarks(
        &self,
        name: &str,
        remarks: &str,
    ) -> Result<(), WerkaPortError> {
        assert_eq!(name, "PR-001");
        self.remarks
            .lock()
            .expect("remarks")
            .push(remarks.to_string());
        Ok(())
    }

    async fn add_notification_delivery_note_comment(
        &self,
        name: &str,
        content: &str,
    ) -> Result<(), WerkaPortError> {
        assert_eq!(name, "DN-001");
        self.delivery_comments
            .lock()
            .expect("comments")
            .push(content.to_string());
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
