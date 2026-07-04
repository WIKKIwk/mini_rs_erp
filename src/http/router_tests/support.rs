use async_trait::async_trait;
use axum::body::to_bytes;

use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::manager::SessionManager;
use crate::core::werka::models::{
    DispatchRecord, SupplierDirectoryEntry, WerkaArchiveResponse, WerkaHomeData, WerkaHomeSummary,
    WerkaStatusBreakdownEntry,
};
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};

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

pub(super) async fn json_body(response: axum::response::Response) -> serde_json::Value {
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

pub(super) struct FakeWerkaHomeLookup;

#[async_trait]
impl WerkaHomeLookup for FakeWerkaHomeLookup {
    async fn werka_summary(&self) -> Result<WerkaHomeSummary, WerkaPortError> {
        Ok(WerkaHomeSummary {
            pending_count: 2,
            confirmed_count: 3,
            returned_count: 1,
        })
    }

    async fn werka_home(&self, pending_limit: usize) -> Result<WerkaHomeData, WerkaPortError> {
        assert_eq!(pending_limit, 20);
        Ok(WerkaHomeData {
            summary: WerkaHomeSummary {
                pending_count: 2,
                confirmed_count: 3,
                returned_count: 1,
            },
            pending_items: Vec::new(),
        })
    }

    async fn werka_pending(&self, limit: usize) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        assert_eq!(limit, 0);
        Ok(vec![DispatchRecord {
            id: "PR-001".to_string(),
            supplier_name: "Supplier".to_string(),
            item_code: "ITEM-001".to_string(),
            item_name: "Item".to_string(),
            uom: "Kg".to_string(),
            sent_qty: 10.0,
            accepted_qty: 0.0,
            status: "pending".to_string(),
            created_label: "2026-01-16".to_string(),
            ..DispatchRecord::default()
        }])
    }

    async fn werka_history(&self) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }

    async fn werka_status_breakdown(
        &self,
        _kind: &str,
    ) -> Result<Vec<WerkaStatusBreakdownEntry>, WerkaPortError> {
        Ok(Vec::new())
    }

    async fn werka_status_details(
        &self,
        _kind: &str,
        _supplier_ref: &str,
    ) -> Result<Vec<DispatchRecord>, WerkaPortError> {
        Ok(Vec::new())
    }

    async fn werka_archive(
        &self,
        _kind: &str,
        _period: &str,
        _from: Option<time::Date>,
        _to: Option<time::Date>,
    ) -> Result<WerkaArchiveResponse, WerkaPortError> {
        Ok(WerkaArchiveResponse::default())
    }

    async fn werka_suppliers(
        &self,
        _: &str,
        _: usize,
        _: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, WerkaPortError> {
        Ok(Vec::new())
    }
}
