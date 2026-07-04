use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};

use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::push::ports::{PushSendError, PushSenderPort};
use crate::core::session::manager::SessionManager;
use crate::core::werka::ports::{
    CatalogItem, CreateDeliveryNoteInput, DeliveryNoteStateUpdate, WerkaCustomerIssueWriter,
    WerkaPortError,
};

pub(super) fn test_state() -> AppState {
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

pub(super) fn request_body() -> &'static str {
    r#"{"customer_ref":"CUST-001","item_code":"ITEM-001","qty":2,"source_barcode":"30AD3353F0C879E4801AD4DF","source_stock_entry":"MAT-STE-2026-00572","source_line_index":1}"#
}

pub(super) fn create_request(token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/mobile/werka/customer-issue/create")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

pub(super) fn batch_request(token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/mobile/werka/customer-issue/batch-create")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

pub(super) async fn json_body(response: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("json")
}

pub(super) async fn werka_session(state: &AppState) -> String {
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

#[derive(Clone, Copy)]
enum FakeIssueMode {
    Ok,
    Duplicate,
    InsufficientStock,
    BatchOk,
    BatchPartial,
}

pub(super) struct FakeIssueWriter {
    mode: FakeIssueMode,
    require_source: bool,
}

impl FakeIssueWriter {
    pub(super) fn ok() -> Self {
        Self {
            mode: FakeIssueMode::Ok,
            require_source: true,
        }
    }

    pub(super) fn duplicate() -> Self {
        Self {
            mode: FakeIssueMode::Duplicate,
            require_source: true,
        }
    }

    pub(super) fn insufficient_stock() -> Self {
        Self {
            mode: FakeIssueMode::InsufficientStock,
            require_source: true,
        }
    }

    pub(super) fn batch_ok() -> Self {
        Self {
            mode: FakeIssueMode::BatchOk,
            require_source: false,
        }
    }

    pub(super) fn batch_partial() -> Self {
        Self {
            mode: FakeIssueMode::BatchPartial,
            require_source: false,
        }
    }
}

#[async_trait]
impl WerkaCustomerIssueWriter for FakeIssueWriter {
    async fn get_items_by_codes(
        &self,
        codes: &[String],
    ) -> Result<Vec<CatalogItem>, WerkaPortError> {
        assert_eq!(codes.len(), 1);
        Ok(vec![CatalogItem {
            code: codes[0].clone(),
            name: format!("{} name", codes[0]),
            uom: "Kg".to_string(),
            item_group: String::new(),
        }])
    }

    async fn resolve_warehouse(&self) -> Result<String, WerkaPortError> {
        Ok("Stores - A".to_string())
    }

    async fn resolve_company(&self) -> Result<String, WerkaPortError> {
        Ok("Accord".to_string())
    }

    async fn customer_issue_source_exists_by_scan(
        &self,
        _customer_ref: &str,
        marker: &str,
    ) -> Result<bool, WerkaPortError> {
        if self.require_source {
            assert!(marker.contains("accord_customer_issue_source:"));
            assert!(marker.contains("source_barcode=30AD3353F0C879E4801AD4DF"));
            assert!(marker.contains("source_stock_entry=MAT-STE-2026-00572"));
            assert!(marker.contains("source_line_index=1"));
        }
        Ok(matches!(self.mode, FakeIssueMode::Duplicate))
    }

    async fn create_draft_delivery_note(
        &self,
        input: CreateDeliveryNoteInput,
    ) -> Result<String, WerkaPortError> {
        assert_eq!(input.customer, "CUST-001");
        if self.require_source {
            assert_eq!(input.item_code, "ITEM-001");
            assert_eq!(input.qty, 2.0);
            assert!(input.source_key.contains("source_line_index=1"));
            Ok("DN-001".to_string())
        } else {
            assert!(input.source_key.is_empty());
            Ok(format!("DN-{}", input.item_code))
        }
    }

    async fn update_delivery_note_state(
        &self,
        name: &str,
        update: DeliveryNoteStateUpdate,
    ) -> Result<(), WerkaPortError> {
        if self.require_source {
            assert_eq!(name, "DN-001");
        }
        assert_eq!(update.flow_state, "1");
        assert_eq!(update.customer_state, "1");
        assert_eq!(update.delivery_actor, "1");
        assert_eq!(update.ui_status, "pending");
        Ok(())
    }

    async fn submit_delivery_note(&self, name: &str) -> Result<(), WerkaPortError> {
        if self.require_source {
            assert_eq!(name, "DN-001");
        }
        if matches!(self.mode, FakeIssueMode::InsufficientStock)
            || (matches!(self.mode, FakeIssueMode::BatchPartial) && name == "DN-ITEM-FAIL")
        {
            Err(WerkaPortError::InsufficientStock)
        } else {
            Ok(())
        }
    }

    async fn delete_delivery_note(&self, name: &str) -> Result<(), WerkaPortError> {
        if self.require_source {
            assert_eq!(name, "DN-001");
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct RecordingPushSender {
    pub(super) calls: Mutex<Vec<PushCall>>,
}

#[derive(Debug)]
pub(super) struct PushCall {
    pub(super) key: String,
    pub(super) title: String,
    pub(super) body: String,
    pub(super) data: HashMap<String, String>,
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
