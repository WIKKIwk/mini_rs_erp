use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::handlers::push_notify::send_dispatch_record;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::PrincipalRole;
use crate::core::push::ports::{PushSendError, PushSenderPort};
use crate::core::push::service::PushService;
use crate::core::werka::models::DispatchRecord;

#[tokio::test]
async fn dispatch_record_push_failure_is_best_effort_like_go() {
    let mut state = test_state();
    state.push =
        PushService::new(state.push.store_for_tests()).with_sender(Arc::new(FailingPushSender));
    let record = DispatchRecord {
        id: "PR-001".to_string(),
        supplier_name: "Supplier".to_string(),
        item_code: "ITEM-001".to_string(),
        item_name: "Rice".to_string(),
        uom: "Kg".to_string(),
        sent_qty: 10.0,
        status: "pending".to_string(),
        created_label: "Bugun".to_string(),
        ..DispatchRecord::default()
    };

    send_dispatch_record(
        &state,
        "werka:werka".to_string(),
        "Title",
        "Body",
        &record,
        PrincipalRole::Werka,
        "werka",
        "best effort test",
    )
    .await;
}

fn test_state() -> AppState {
    AppState::new(AppConfig {
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
    })
}

struct FailingPushSender;

#[async_trait]
impl PushSenderPort for FailingPushSender {
    async fn send_to_key(
        &self,
        _key: &str,
        _title: &str,
        _body: &str,
        _data: HashMap<String, String>,
    ) -> Result<(), PushSendError> {
        Err(PushSendError::SendFailed)
    }
}
