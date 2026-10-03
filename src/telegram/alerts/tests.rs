use super::models::AlertMember;
use super::{AlertKind, AlertSenderUpdate, TelegramAlertSettings};
use crate::telegram::{
    TelegramAccountRole, TelegramDeliveryMode, TelegramService, TelegramUserAccount,
    TelegramUserGroup,
};

fn account(id: &str, role: TelegramAccountRole) -> TelegramUserAccount {
    TelegramUserAccount {
        telegram_user_id: id.into(),
        telegram_chat_id: id.into(),
        username: String::new(),
        display_name: format!("User {id}"),
        role,
        invite_token: String::new(),
        joined_at_unix: 1,
        phone_number: String::new(),
        delivery_mode: TelegramDeliveryMode::UserProfile,
        user_profile_connected: true,
        selected_chat_id: Some("order-group".into()),
        selected_chat_title: Some("Orders".into()),
        selected_chat_type: Some("supergroup".into()),
    }
}

fn group(id: &str) -> TelegramUserGroup {
    TelegramUserGroup {
        chat_id: id.into(),
        title: "Printing".into(),
        chat_type: "supergroup".into(),
        username: String::new(),
    }
}

fn member(id: i64) -> AlertMember {
    AlertMember {
        user_id: id,
        display_name: format!("Mas’ul {id}"),
        username: String::new(),
        access_hash: Some(id + 100),
    }
}

async fn linked(service: &TelegramService, id: &str, role: TelegramAccountRole) {
    service
        .store
        .register_qr_user(account(id, role), "test-session".into())
        .await
        .unwrap()
        .unwrap();
}

async fn configured(service: &TelegramService) {
    linked(service, "101", TelegramAccountRole::SalesManager).await;
    service
        .update_alert_sender(AlertSenderUpdate {
            sender_user_id: Some("101".into()),
        })
        .await
        .unwrap();
    service
        .store
        .set_alert_group("101", group("111"))
        .await
        .unwrap();
    let settings = service.store.alert_settings().await;
    service
        .store
        .toggle_alert_member(&settings, AlertKind::RawMaterial, member(201))
        .await
        .unwrap();
    service
        .store
        .toggle_alert_member(&settings, AlertKind::Qolip, member(301))
        .await
        .unwrap();
}

#[tokio::test]
async fn old_store_and_missing_configuration_keep_alerts_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("telegram.json");
    std::fs::write(&path, "{\"users\":{},\"chats\":{}}").unwrap();
    let service = TelegramService::new(path);
    assert_eq!(
        service.admin_overview().await.unwrap().alerts,
        TelegramAlertSettings::default()
    );
    assert!(
        !service
            .enqueue_order_alert("request".into(), AlertKind::RawMaterial, "Order".into())
            .await
            .unwrap()
    );
    assert!(service.store.due_alert(i64::MAX).await.is_none());
    assert!(
        service
            .update_alert_sender(AlertSenderUpdate {
                sender_user_id: Some("unknown".into())
            })
            .await
            .is_err()
    );
    let mut unlinked = account("102", TelegramAccountRole::Admin);
    unlinked.user_profile_connected = false;
    service
        .store
        .register_qr_user(unlinked, "test".into())
        .await
        .unwrap();
    assert!(
        service
            .update_alert_sender(AlertSenderUpdate {
                sender_user_id: Some("102".into())
            })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn notifier_authorization_and_selection_do_not_change_order_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let service = TelegramService::new(dir.path().join("telegram.json"));
    configured(&service).await;
    linked(&service, "102", TelegramAccountRole::SalesManager).await;
    linked(&service, "103", TelegramAccountRole::Admin).await;
    assert!(service.authorize_alert_configuration("101").await.is_ok());
    assert!(service.authorize_alert_configuration("103").await.is_ok());
    assert!(service.authorize_alert_configuration("102").await.is_err());
    assert!(
        service
            .authorize_alert_configuration("unknown")
            .await
            .is_err()
    );
    let original = service.store.alert_settings().await;
    service
        .update_alert_sender(AlertSenderUpdate {
            sender_user_id: Some(" 101 ".into()),
        })
        .await
        .unwrap();
    assert_eq!(service.store.alert_settings().await, original);
    service
        .update_alert_sender(AlertSenderUpdate {
            sender_user_id: Some("102".into()),
        })
        .await
        .unwrap();
    let updated = service.store.alert_settings().await;
    assert_eq!(updated.sender_user_id.as_deref(), Some("102"));
    assert!(updated.group.is_none());
    assert!(updated.raw_material_members.is_empty() && updated.qolip_members.is_empty());
    assert_eq!(
        service
            .user_by_telegram_id("101")
            .await
            .unwrap()
            .unwrap()
            .selected_chat_id
            .as_deref(),
        Some("order-group")
    );
    service
        .update_alert_sender(AlertSenderUpdate {
            sender_user_id: None,
        })
        .await
        .unwrap();
    assert_eq!(
        service.store.alert_settings().await,
        TelegramAlertSettings::default()
    );
}

#[tokio::test]
async fn group_change_resets_roles_and_stale_choices_cannot_modify_new_group() {
    let dir = tempfile::tempdir().unwrap();
    let service = TelegramService::new(dir.path().join("telegram.json"));
    configured(&service).await;
    let old = service.store.alert_settings().await;
    service
        .store
        .set_alert_group("101", group("111"))
        .await
        .unwrap();
    assert_eq!(service.store.alert_settings().await, old);
    service
        .store
        .set_alert_group("101", group("222"))
        .await
        .unwrap();
    assert!(
        service
            .store
            .toggle_alert_member(&old, AlertKind::Qolip, member(401))
            .await
            .is_err()
    );
    let current = service.store.alert_settings().await;
    assert!(current.raw_material_members.is_empty() && current.qolip_members.is_empty());
    service
        .store
        .toggle_alert_member(&current, AlertKind::Qolip, member(401))
        .await
        .unwrap();
    service
        .store
        .toggle_alert_member(&current, AlertKind::Qolip, member(401))
        .await
        .unwrap();
    assert!(
        service
            .store
            .alert_settings()
            .await
            .qolip_members
            .is_empty()
    );
    assert!(
        service
            .store
            .set_alert_group("wrong-sender", group("333"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn alerts_route_by_kind_and_survive_restart_and_retry_without_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("telegram.json");
    let service = TelegramService::new(path.clone());
    configured(&service).await;
    for _ in 0..2 {
        assert!(
            service
                .enqueue_order_alert(
                    "request-material".into(),
                    AlertKind::RawMaterial,
                    "№12 homashyo".into()
                )
                .await
                .unwrap()
        );
    }
    let first = service.store.due_alert(i64::MAX).await.unwrap();
    assert_eq!(first.members, vec![member(201)]);
    assert_eq!(first.sender_user_id, "101");
    assert_ne!(first.random_id, 0);
    service
        .store
        .finish_alert(&first.id, Some("network".into()), 500)
        .await
        .unwrap();
    let restarted = TelegramService::new(path.clone());
    assert!(restarted.store.due_alert(499).await.is_none());
    let retry = restarted.store.due_alert(500).await.unwrap();
    assert_eq!(retry.random_id, first.random_id);
    assert_eq!(retry.attempts, 1);
    assert_eq!(retry.members, first.members);
    restarted
        .store
        .finish_alert(&retry.id, None, 0)
        .await
        .unwrap();
    assert!(
        restarted
            .enqueue_order_alert(first.id, AlertKind::RawMaterial, "same request".into())
            .await
            .unwrap()
    );
    assert!(restarted.store.due_alert(i64::MAX).await.is_none());
    restarted
        .enqueue_order_alert("request-mold".into(), AlertKind::Qolip, "№12 qolip".into())
        .await
        .unwrap();
    let mold = restarted.store.due_alert(i64::MAX).await.unwrap();
    assert_eq!(mold.members, vec![member(301)]);
    assert_ne!(mold.random_id, first.random_id);
    let data: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(data["alert_jobs"].as_object().unwrap().len(), 2);
}

#[tokio::test]
async fn unconfigured_role_does_not_send_and_deleted_notifier_clears_pending_alerts() {
    let dir = tempfile::tempdir().unwrap();
    let service = TelegramService::new(dir.path().join("telegram.json"));
    configured(&service).await;
    let settings = service.store.alert_settings().await;
    service
        .store
        .toggle_alert_member(&settings, AlertKind::Qolip, member(301))
        .await
        .unwrap();
    assert!(
        !service
            .enqueue_order_alert("mold".into(), AlertKind::Qolip, "mold".into())
            .await
            .unwrap()
    );
    service
        .enqueue_order_alert("material".into(), AlertKind::RawMaterial, "material".into())
        .await
        .unwrap();
    service.delete_user_account("101").await.unwrap();
    assert_eq!(
        service.store.alert_settings().await,
        TelegramAlertSettings::default()
    );
    assert!(service.store.due_alert(i64::MAX).await.is_none());
}

#[tokio::test]
async fn telegram_alert_addresses_selected_members_without_emoji_or_section_headers() {
    use ferogram::tl;
    let dir = tempfile::tempdir().unwrap();
    let service = TelegramService::new(dir.path().join("telegram.json"));
    configured(&service).await;
    let text = "№0023 guruch alanga arzon 1kg buyurtmasiga homashyo biriktirib bering, 8 ta rangli bosma aparat'dagi Nuriddin aka kutyapti";
    service.enqueue_order_alert("plain".into(), AlertKind::RawMaterial, text.into()).await.unwrap();
    let job = service.store.due_alert(i64::MAX).await.unwrap();
    let request = crate::telegram::useraccount::alerts::alert_request(tl::enums::InputPeer::PeerSelf, &job);
    assert_eq!(request.message, format!("Mas’ul 201, {text}"));
    assert!(!request.message.contains("⚠️"));
    assert!(!request.message.contains("Mas’ullar:"));
    assert!(!request.message.contains('\n'));
    let tl::enums::MessageEntity::InputMessageEntityMentionName(entity) = &request.entities.unwrap()[0] else { panic!("ID mention required"); };
    assert_eq!(entity.offset, 0);
    assert_eq!(entity.length, "Mas’ul 201".encode_utf16().count() as i32);
    let tl::enums::InputUser::InputUser(user) = &entity.user_id else { panic!("InputUser required"); };
    assert_eq!(user.user_id, 201);
}

#[tokio::test]
async fn message_mentions_user_ids_with_utf16_offsets_and_bounded_length() {
    use ferogram::tl;
    let dir = tempfile::tempdir().unwrap();
    let service = TelegramService::new(dir.path().join("telegram.json"));
    configured(&service).await;
    let settings = service.store.alert_settings().await;
    for id in 202..231 {
        let mut user = member(id);
        user.display_name = "🧑‍🔧".repeat(100);
        service
            .store
            .toggle_alert_member(&settings, AlertKind::RawMaterial, user)
            .await
            .unwrap();
    }
    assert!(
        service
            .store
            .toggle_alert_member(&settings, AlertKind::RawMaterial, member(231))
            .await
            .is_err()
    );
    service
        .enqueue_order_alert(
            "long".into(),
            AlertKind::RawMaterial,
            "📦 homashyo ".repeat(500),
        )
        .await
        .unwrap();
    let job = service.store.due_alert(i64::MAX).await.unwrap();
    let request =
        crate::telegram::useraccount::alerts::alert_request(tl::enums::InputPeer::PeerSelf, &job);
    assert_eq!(request.random_id, job.random_id);
    assert!(request.message.encode_utf16().count() <= 4096);
    let encoded: Vec<u16> = request.message.encode_utf16().collect();
    let entities = request.entities.unwrap();
    assert_eq!(entities.len(), 30);
    for (entity, member) in entities.into_iter().zip(&job.members) {
        let tl::enums::MessageEntity::InputMessageEntityMentionName(entity) = entity else {
            panic!("ID mention required")
        };
        let tl::enums::InputUser::InputUser(user) = entity.user_id else {
            panic!("InputUser required")
        };
        assert_eq!(user.user_id, member.user_id);
        assert_eq!(user.access_hash, member.access_hash.unwrap());
        let label = String::from_utf16(
            &encoded[entity.offset as usize..(entity.offset + entity.length) as usize],
        )
        .unwrap();
        assert!(!label.is_empty());
        assert!(member.display_name.starts_with(&label));
    }
}

#[tokio::test]
async fn alert_sender_invite_registers_dedicated_role_and_login_activates_sender() {
    use crate::telegram::{TelegramBotSettingsUpdate, TelegramInviteRequest, TelegramStartRequest};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("telegram.json");
    let service = TelegramService::new(path.clone());
    service
        .update_bot_settings(TelegramBotSettingsUpdate {
            bot_username: "accord_bot".into(),
            bot_token: "test".into(),
        })
        .await
        .unwrap();
    let invite = service
        .create_invite(TelegramInviteRequest {
            role: TelegramAccountRole::AlertSender,
        })
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&invite).unwrap()["role"],
        "alert_sender"
    );
    let token = invite.invite_url.split_once("start=").unwrap().1;
    let user = service
        .register_bot_start(TelegramStartRequest {
            invite_token: token.into(),
            telegram_user_id: "501".into(),
            telegram_chat_id: "501".into(),
            username: String::new(),
            display_name: "Notifier".into(),
        })
        .await
        .unwrap();
    assert_eq!(user.role, TelegramAccountRole::AlertSender);
    assert_eq!(user.delivery_mode, TelegramDeliveryMode::UserProfile);
    assert!(!user.user_profile_connected);
    assert!(
        service
            .store
            .alert_settings()
            .await
            .sender_user_id
            .is_none()
    );
    assert!(service.authorize_alert_configuration("501").await.is_err());
    service
        .store
        .complete_user_profile_login("501", "998901234567".into(), "authenticated-session".into())
        .await
        .unwrap();
    let restarted = TelegramService::new(path);
    assert_eq!(
        restarted
            .store
            .alert_settings()
            .await
            .sender_user_id
            .as_deref(),
        Some("501")
    );
    assert!(restarted.authorize_alert_configuration("501").await.is_ok());
    assert_eq!(
        restarted
            .user_by_telegram_id("501")
            .await
            .unwrap()
            .unwrap()
            .role,
        TelegramAccountRole::AlertSender
    );
    assert_eq!(
        restarted
            .store
            .user_session("501")
            .await
            .unwrap()
            .as_deref(),
        Some("authenticated-session")
    );
}

#[tokio::test]
async fn qr_alert_sender_is_selected_automatically_without_admin_or_sales_role() {
    let dir = tempfile::tempdir().unwrap();
    let service = TelegramService::new(dir.path().join("telegram.json"));
    linked(&service, "501", TelegramAccountRole::AlertSender).await;
    assert_eq!(
        service
            .store
            .alert_settings()
            .await
            .sender_user_id
            .as_deref(),
        Some("501")
    );
    assert!(service.authorize_alert_configuration("501").await.is_ok());
    service
        .store
        .set_alert_group("501", group("111"))
        .await
        .unwrap();
    let configured = service.store.alert_settings().await;
    service
        .store
        .complete_user_profile_login("501", String::new(), "renewed-session".into())
        .await
        .unwrap();
    assert_eq!(service.store.alert_settings().await, configured);
    linked(&service, "502", TelegramAccountRole::AlertSender).await;
    assert_eq!(
        service
            .store
            .alert_settings()
            .await
            .sender_user_id
            .as_deref(),
        Some("502")
    );
    assert!(service.store.alert_settings().await.group.is_none());
    assert!(service.authorize_alert_configuration("501").await.is_err());
}
