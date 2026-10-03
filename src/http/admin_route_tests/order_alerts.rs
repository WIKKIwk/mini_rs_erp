use super::order_alerts_chat::RecordingChatStore;
use super::*;
use crate::core::authz::RoleAssignmentUpsert;
use crate::core::chat::ChatService;
use crate::core::system_users::SystemUserUpsert;

const APPARATUS: &str = "apparatus:default:bosma_7";
const ENDPOINT: &str = "/v1/mobile/admin/production-maps/order-alert";

async fn fixture() -> (
    AppState,
    String,
    Arc<RecordingChatStore>,
    Arc<FakeAdminStatePort>,
    tempfile::TempDir,
) {
    let mut state = test_state();
    let telegram_dir = tempfile::tempdir().unwrap();
    state.telegram = crate::telegram::TelegramService::new(telegram_dir.path().join("telegram.json"));
    let admin_state = Arc::new(FakeAdminStatePort::new());
    state.admin = AdminService::new(&state.config)
        .with_read_port(Arc::new(FakeAdminReadPort))
        .with_state_port(admin_state.clone());
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "alert-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![APPARATUS.into()],
            assigned_item_groups: vec![],
        })
        .await
        .expect("assignment");
    let chat = Arc::new(RecordingChatStore::default());
    state.chat = ChatService::new(chat.clone());
    let admin = session(&state, PrincipalRole::Admin).await;
    let saved = build_router(state.clone()).oneshot(request_with_body(
        "PUT", "/v1/mobile/admin/production-maps", &admin,
        &serde_json::json!({
            "id":"zakaz-alert", "order_number":"123", "product_code":"ALERT", "title":"Alert order",
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"print","kind":"apparatus","title":APPARATUS,"apparatus_id":APPARATUS},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"print"},{"from":"print","to":"end"}]
        }).to_string(),
    )).await.expect("map response");
    assert_eq!(
        saved.status(),
        StatusCode::OK,
        "{:?}",
        json_body(saved).await
    );
    let worker = session_for(&state, PrincipalRole::Aparatchi, "alert-worker").await;
    (state, worker, chat, admin_state, telegram_dir)
}

fn command(kind: &str) -> String {
    serde_json::json!({"order_id":"zakaz-alert", "apparatus":APPARATUS,
        "kind":kind, "request_id":"reminder-1"})
    .to_string()
}

async fn add_qolipchi(state: &AppState, id: &str, phone: &str) {
    state
        .system_users
        .upsert_user(SystemUserUpsert {
            id: id.into(),
            role: PrincipalRole::Qolipchi,
            name: id.into(),
            phone: phone.into(),
        })
        .await
        .expect("qolipchi");
}

#[tokio::test]
async fn order_alert_material_routes_by_role_and_reuses_retry_id() {
    let (state, worker, chat, _, _telegram_dir) = fixture().await;
    for _ in 0..2 {
        let response = build_router(state.clone())
            .oneshot(request_with_body(
                "POST",
                ENDPOINT,
                &worker,
                &command("raw_material"),
            ))
            .await
            .expect("alert response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await["recipient_count"], 1);
    }
    assert_eq!(
        chat.recipients.lock().await.get("MAT-NEW"),
        Some(&PrincipalRole::MaterialTaminotchi)
    );
    let messages = chat.messages.lock().await;
    assert_eq!(messages.len(), 1);
    let message = messages.values().next().unwrap();
    assert!(
        message
            .body
            .starts_with("№123 orderga homashyo biriktirib bering."),
        "{}",
        message.body
    );
    assert!(message.body.contains("Apparat:"));
    assert!(message.body.contains("Ishchi: Admin"));
    assert_eq!(message.sender_ref, "alert-worker");
}

#[tokio::test]
async fn order_alert_qolip_skips_blocked_and_removed_recipients() {
    let (state, worker, chat, admin_state, _telegram_dir) = fixture().await;
    for (id, phone) in [
        ("active", "+998901111111"),
        ("blocked", "+998901111112"),
        ("removed", "+998901111113"),
    ] {
        add_qolipchi(&state, id, phone).await;
    }
    admin_state
        .put_state(
            "blocked",
            AdminState {
                blocked: true,
                ..AdminState::default()
            },
        )
        .await
        .unwrap();
    admin_state
        .put_state(
            "removed",
            AdminState {
                removed: true,
                ..AdminState::default()
            },
        )
        .await
        .unwrap();
    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            ENDPOINT,
            &worker,
            &command("qolip"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["recipient_count"], 1);
    assert_eq!(
        *chat.recipients.lock().await,
        BTreeMap::from([("active".into(), PrincipalRole::Qolipchi)])
    );
    assert!(
        chat.messages
            .lock()
            .await
            .values()
            .next()
            .unwrap()
            .body
            .contains("orderga qolip biriktirib bering.")
    );
}

#[tokio::test]
async fn order_alert_partial_failure_can_retry_without_duplicate_messages() {
    let (state, worker, chat, _, _telegram_dir) = fixture().await;
    add_qolipchi(&state, "a-first", "+998901111111").await;
    add_qolipchi(&state, "z-second", "+998901111112").await;
    *chat.fail_recipient.lock().await = Some("z-second".into());
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            ENDPOINT,
            &worker,
            &command("qolip"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json_body(response).await["error"],
        "order_alert_send_failed"
    );
    assert_eq!(chat.messages.lock().await.len(), 1);
    *chat.fail_recipient.lock().await = None;
    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            ENDPOINT,
            &worker,
            &command("qolip"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["recipient_count"], 2);
    assert_eq!(chat.messages.lock().await.len(), 2);
}

#[tokio::test]
async fn order_alert_reports_missing_recipients_and_rejects_invalid_targets() {
    let (state, worker, chat, _, _telegram_dir) = fixture().await;
    let other = session_for(&state, PrincipalRole::Aparatchi, "other-worker").await;
    let admin = session(&state, PrincipalRole::Admin).await;
    for token in [other, admin] {
        let response = build_router(state.clone())
            .oneshot(request_with_body(
                "POST",
                ENDPOINT,
                &token,
                &command("raw_material"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            ENDPOINT,
            &worker,
            &command("qolip"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await["error"],
        "order_alert_no_recipients"
    );
    let mut invalid: serde_json::Value = serde_json::from_str(&command("raw_material")).unwrap();
    invalid["apparatus"] = "apparatus:default:bosma_8".into();
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            ENDPOINT,
            &worker,
            &invalid.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    invalid["apparatus"] = APPARATUS.into();
    invalid["order_id"] = "missing-order".into();
    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            ENDPOINT,
            &worker,
            &invalid.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(chat.messages.lock().await.is_empty());
}

async fn configure_telegram_alerts(state: &AppState) {
    use crate::telegram::alerts::models::AlertMember;
    use crate::telegram::alerts::{AlertKind, AlertSenderUpdate};
    use crate::telegram::{TelegramUserAccount, TelegramUserGroup};
    let user: TelegramUserAccount = serde_json::from_value(serde_json::json!({
        "telegram_user_id":"101", "username":"notifier", "display_name":"Notifier",
        "role":"sales_manager", "invite_token":"", "joined_at_unix":1,
        "delivery_mode":"user_profile", "user_profile_connected":true
    }))
    .unwrap();
    state
        .telegram
        .store
        .register_qr_user(user, "test-session".into())
        .await
        .unwrap();
    state
        .telegram
        .update_alert_sender(AlertSenderUpdate {
            sender_user_id: Some("101".into()),
        })
        .await
        .unwrap();
    state
        .telegram
        .store
        .set_alert_group(
            "101",
            TelegramUserGroup {
                chat_id: "111".into(),
                title: "Printing".into(),
                chat_type: "supergroup".into(),
                username: String::new(),
            },
        )
        .await
        .unwrap();
    let settings = state.telegram.store.alert_settings().await;
    for (kind, id) in [(AlertKind::RawMaterial, 201), (AlertKind::Qolip, 301)] {
        state
            .telegram
            .store
            .toggle_alert_member(
                &settings,
                kind,
                AlertMember {
                    user_id: id,
                    display_name: format!("Member {id}"),
                    username: String::new(),
                    access_hash: Some(id + 100),
                },
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn order_alert_queues_telegram_by_role_with_dedup_and_internal_delivery() {
    let (mut state, worker, chat, _, _telegram_dir) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("telegram.json");
    state.telegram = crate::telegram::TelegramService::new(path.clone());
    configure_telegram_alerts(&state).await;
    for kind in ["raw_material", "raw_material", "qolip"] {
        let response = build_router(state.clone())
            .oneshot(request_with_body("POST", ENDPOINT, &worker, &command(kind)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["telegram_queued"], true);
        assert_eq!(
            body["recipient_count"],
            if kind == "raw_material" { 1 } else { 0 }
        );
    }
    assert_eq!(chat.messages.lock().await.len(), 1);
    let persisted: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let jobs = persisted["alert_jobs"].as_object().unwrap();
    assert_eq!(jobs.len(), 2);
    for job in jobs.values() {
        assert_eq!(job["sender_user_id"], "101");
        assert_eq!(job["group"]["chat_id"], "111");
        assert_eq!(job["members"].as_array().unwrap().len(), 1);
        let message = job["message"].as_str().unwrap();
        let item = if message.contains("homashyo") { "homashyo" } else { "qolip" };
        assert!(message.starts_with(&format!(
            "№123 Alert order buyurtmasiga {item} biriktirib bering, "
        )));
        assert!(message.ends_with("'dagi Admin aka kutyapti"));
        assert!(!message.contains('\n'));
        assert_eq!(
            job["members"][0]["user_id"],
            if job["message"].as_str().unwrap().contains("homashyo") {
                201
            } else {
                301
            }
        );
    }
}

#[tokio::test]
async fn telegram_alert_sender_endpoint_requires_admin_settings_permission() {
    let (mut state, worker, _, _, _telegram_dir) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    state.telegram = crate::telegram::TelegramService::new(dir.path().join("telegram.json"));
    configure_telegram_alerts(&state).await;
    let endpoint = "/v1/mobile/admin/telegram/alert-settings";
    let body = "{\"sender_user_id\":null}";
    let denied = build_router(state.clone())
        .oneshot(request_with_body("PUT", endpoint, &worker, body))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let admin = session(&state, PrincipalRole::Admin).await;
    let invalid = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            endpoint,
            &admin,
            "{\"sender_user_id\":\"unknown\"}",
        ))
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        state
            .telegram
            .store
            .alert_settings()
            .await
            .sender_user_id
            .as_deref(),
        Some("101")
    );
    let saved = build_router(state.clone())
        .oneshot(request_with_body("PUT", endpoint, &admin, body))
        .await
        .unwrap();
    assert_eq!(saved.status(), StatusCode::OK);
    assert!(json_body(saved).await["alerts"]["sender_user_id"].is_null());
    assert!(state.telegram.store.alert_settings().await.group.is_none());
}

#[tokio::test]
async fn telegram_alert_sender_invite_endpoint_returns_dedicated_role_link() {
    let mut state = test_state();
    let dir = tempfile::tempdir().unwrap();
    state.telegram = crate::telegram::TelegramService::new(dir.path().join("telegram.json"));
    state
        .telegram
        .update_bot_settings(crate::telegram::TelegramBotSettingsUpdate {
            bot_username: "accord_bot".into(),
            bot_token: "test".into(),
        })
        .await
        .unwrap();
    let admin = session(&state, PrincipalRole::Admin).await;
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/telegram/invites",
            &admin,
            "{\"role\":\"alert_sender\"}",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["role"], "alert_sender");
    let link = body["invite_url"].as_str().unwrap();
    assert!(link.starts_with("https://t.me/accord_bot?start="));
    let user = state
        .telegram
        .register_bot_start(crate::telegram::TelegramStartRequest {
            invite_token: link.split_once("start=").unwrap().1.into(),
            telegram_user_id: "501".into(),
            telegram_chat_id: "501".into(),
            username: String::new(),
            display_name: "Notifier".into(),
        })
        .await
        .unwrap();
    assert_eq!(user.role, crate::telegram::TelegramAccountRole::AlertSender);
    assert!(!user.user_profile_connected);
}
