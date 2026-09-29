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
) {
    let mut state = test_state();
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
    (state, worker, chat, admin_state)
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
    let (state, worker, chat, _) = fixture().await;
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
    let (state, worker, chat, admin_state) = fixture().await;
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
    let (state, worker, chat, _) = fixture().await;
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
    let (state, worker, chat, _) = fixture().await;
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
