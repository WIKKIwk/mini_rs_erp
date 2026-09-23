use super::*;
use crate::telegram::store::TelegramStore;

fn qr_account(role: TelegramAccountRole) -> TelegramUserAccount {
    serde_json::from_value(serde_json::json!({
        "telegram_user_id": "123", "telegram_chat_id": "",
        "username": "qr_user", "display_name": "QR User", "role": role,
        "invite_token": "", "joined_at_unix": 1,
        "delivery_mode": "user_profile", "user_profile_connected": true
    }))
    .unwrap()
}

#[tokio::test]
async fn side_selection_survives_restart_and_preserves_the_order_image() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("telegram.json");
    let service = TelegramService::new(path.clone());
    let mut draft = TelegramOrderDraft::default();
    draft.request_side();
    draft.prompt_message_id = Some(10);
    draft.side_prompt_message_id = Some(11);
    service.save_order_draft("123", draft.clone()).await.unwrap();
    service.save_order_attachment("123", TelegramOrderAttachment {
        file_name: "customer-order.png".into(),
        mime_type: "image/png".into(),
        body: vec![1, 2, 3],
    }).await;
    let restarted = TelegramService::new(path.clone());
    let mut restored = restarted.order_draft("123").await.unwrap().unwrap();
    assert_eq!(restored, draft);
    assert!(restored.select_side("3"));
    service.save_order_draft("123", restored).await.unwrap();
    let restored = TelegramService::new(path).order_draft("123").await.unwrap().unwrap();
    assert_eq!(restored.side, Some(3));
    assert_eq!(restored.step, TelegramOrderStep::Review);
    assert_eq!(restored.prompt_message_id, Some(10));
    assert_eq!(service.order_attachment("123").await.unwrap().file_name, "customer-order.png");
}

#[tokio::test]
async fn qr_login_start_and_group_selection_use_the_saved_profile() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("telegram.json");
    let store = TelegramStore::new(path.clone());
    store
        .register_qr_user(
            qr_account(TelegramAccountRole::SalesManager),
            "saved-mtproto-session".into(),
        )
        .await
        .unwrap()
        .unwrap();
    let service = TelegramService::new(path.clone());
    let start = || TelegramStartRequest {
        invite_token: String::new(),
        telegram_user_id: "123".into(),
        telegram_chat_id: "123".into(),
        username: "renamed_user".into(),
        display_name: "Renamed User".into(),
    };
    let account = service.register_bot_start(start()).await.unwrap();
    assert!(account.user_profile_connected);
    assert_eq!(account.telegram_chat_id, "123");
    assert_eq!(account.delivery_mode, TelegramDeliveryMode::UserProfile);
    assert!(account_guide(&account).contains("User profile orqali login qilgansiz"));
    assert!(account_guide(&account).contains("/groups"));
    assert_eq!(
        account_guide_keyboard(&account).unwrap()["inline_keyboard"][0][0]["callback_data"],
        "user_groups"
    );

    // Group selection stores the target and activates user-profile delivery,
    // including when this account had previously chosen bot delivery.
    let store = TelegramStore::new(path.clone());
    store
        .set_delivery_mode("123", TelegramDeliveryMode::Bot)
        .await
        .unwrap();
    store
        .set_selected_user_group(
            "123",
            TelegramUserGroup {
                chat_id: "777".into(),
                title: "Order group".into(),
                chat_type: "supergroup".into(),
                username: String::new(),
            },
        )
        .await
        .unwrap();
    let restarted = TelegramService::new(path.clone());
    let account = restarted.register_bot_start(start()).await.unwrap();
    assert_eq!(account.selected_chat_id.as_deref(), Some("777"));
    assert_eq!(account.delivery_mode, TelegramDeliveryMode::UserProfile);
    assert!(account_guide(&account).contains("Order group"));
    assert_eq!(
        TelegramStore::new(path)
            .user_session("123")
            .await
            .unwrap()
            .as_deref(),
        Some("saved-mtproto-session")
    );
}

#[test]
fn connected_admin_can_pick_groups_and_unconnected_user_keeps_login_options() {
    let account = qr_account(TelegramAccountRole::Admin);
    assert!(account_guide(&account).contains("/groups"));
    assert!(account_guide_keyboard(&account).is_some());
    let mut account = qr_account(TelegramAccountRole::SalesManager);
    account.user_profile_connected = false;
    account.delivery_mode = TelegramDeliveryMode::Bot;
    assert!(!account_guide(&account).contains("User profile orqali login qilgansiz"));
    assert_eq!(
        account_guide_keyboard(&account),
        Some(delivery_mode_keyboard())
    );
}

#[tokio::test]
async fn start_reaches_profile_recognition_while_order_draft_is_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("telegram.json");
    let store = TelegramStore::new(path.clone());
    store
        .register_qr_user(
            qr_account(TelegramAccountRole::SalesManager),
            "saved-session".into(),
        )
        .await
        .unwrap();
    store
        .save_order_draft("123", TelegramOrderDraft::default())
        .await
        .unwrap();
    let service = TelegramService::new(path);
    let message: TelegramMessage = serde_json::from_value(serde_json::json!({
        "chat": {"id": 123, "type": "private"},
        "from": {"id": 123, "first_name": "QR User"},
        "text": "/start"
    }))
    .unwrap();
    assert!(
        !handle_private_text(&service, "unused", &message, "/start")
            .await
            .unwrap()
    );
    assert!(service.order_draft("123").await.unwrap().is_some());
}
