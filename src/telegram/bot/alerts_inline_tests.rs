use super::*;
use crate::telegram::alerts::TelegramAlertSettings;

fn group(id: usize, title: &str) -> TelegramUserGroup {
    TelegramUserGroup {
        chat_id: id.to_string(),
        title: title.into(),
        chat_type: "supergroup".into(),
        username: format!("group{id}"),
    }
}

fn member(id: i64) -> AlertMember {
    AlertMember {
        user_id: id,
        display_name: format!("Mas’ul {id}"),
        username: String::new(),
        access_hash: Some(id + 10),
    }
}

#[test]
fn inline_alert_selections_accept_callbacks_without_a_chat_message() {
    for action in ["alert:group:opaque-token", "alert:toggle:opaque-token"] {
        let callback: TelegramCallbackQuery = serde_json::from_value(serde_json::json!({
            "id": "callback", "from": {"id": 101}, "data": action,
            "inline_message_id": "telegram-inline-message"
        }))
        .unwrap();
        assert!(callback.message.is_none());
        assert!(alert_callback_is_allowed(&callback));
    }
    for data in ["alert:home", "alert:members:qolip:0", "other:action"] {
        let callback: TelegramCallbackQuery = serde_json::from_value(serde_json::json!({
            "id": "callback", "from": {"id": 101}, "data": data,
            "inline_message_id": "telegram-inline-message"
        }))
        .unwrap();
        assert!(!alert_callback_is_allowed(&callback));
    }
    let callback: TelegramCallbackQuery = serde_json::from_value(serde_json::json!({
        "id": "callback", "from": {"id": 101}, "data": "alert:group:opaque-token",
        "message": {"message_id": 7, "chat": {"id": -10, "type": "supergroup"}}
    }))
    .unwrap();
    assert!(!alert_callback_is_allowed(&callback));
}

#[test]
fn alert_pickers_use_separate_inline_searches_and_keep_login_ordered() {
    for (prefix, expected) in [
        (INLINE_ALERT_GROUP_PREFIX, AlertInlineSearch::Groups("")),
        (
            INLINE_ALERT_MATERIAL_PREFIX,
            AlertInlineSearch::Members(AlertKind::RawMaterial, ""),
        ),
        (
            INLINE_ALERT_QOLIP_PREFIX,
            AlertInlineSearch::Members(AlertKind::Qolip, ""),
        ),
    ] {
        assert_eq!(parse_alert_inline_query(prefix), Some(expected));
        let query: TelegramInlineQuery =
            serde_json::from_value(serde_json::json!({"id":"q", "from":{"id":1}, "query": prefix}))
                .unwrap();
        assert!(is_inline_search(&query));
        assert_eq!(query.offset, "");
    }
    assert_eq!(
        parse_alert_inline_query("  am7  @Ali  "),
        Some(AlertInlineSearch::Members(AlertKind::RawMaterial, "@Ali"))
    );
    assert_eq!(parse_alert_inline_query("aq7extra"), None);
    for query in ["q7 12345", "p4 password", "login", "unknown"] {
        let query: TelegramInlineQuery =
            serde_json::from_value(serde_json::json!({"id":"q", "from":{"id":1}, "query": query}))
                .unwrap();
        assert!(!is_inline_search(&query));
    }
    for kind in [None, Some(AlertKind::RawMaterial), Some(AlertKind::Qolip)] {
        let rows = alert_search_rows(kind);
        assert!(
            parse_alert_inline_query(
                rows[0][0]["switch_inline_query_current_chat"]
                    .as_str()
                    .unwrap()
            )
            .is_some()
        );
        assert_eq!(rows[1][0]["callback_data"], "alert:home");
        assert!(rows[0][0].get("callback_data").is_none());
    }
}

#[tokio::test]
async fn inline_group_results_filter_paginate_and_bind_choices_to_actor() {
    let dir = tempfile::tempdir().unwrap();
    let service = TelegramService::new(dir.path().join("telegram.json"));
    let settings = TelegramAlertSettings {
        sender_user_id: Some("101".into()),
        group: Some(group(1, "Printing 01")),
        ..Default::default()
    };
    let groups: Vec<_> = (1..=25)
        .map(|id| group(id, &format!("Printing {id:02}")))
        .collect();
    let (articles, next) =
        alert_group_articles(&service, "101", &settings, groups.clone(), "PRINTING", 0)
            .await
            .unwrap();
    assert_eq!(articles.len(), 20);
    assert_eq!(next, "20");
    assert_eq!(articles[0]["title"], "✅ Printing 01");
    let data = articles[0]["reply_markup"]["inline_keyboard"][0][0]["callback_data"]
        .as_str()
        .unwrap();
    assert!(data.len() <= 64);
    let token = data.strip_prefix("alert:group:").unwrap();
    assert!(
        service
            .take_order_choice("other-actor", token)
            .await
            .is_none()
    );
    let choice: AlertChoice =
        serde_json::from_str(&service.take_order_choice("101", token).await.unwrap()).unwrap();
    assert_eq!(choice.sender, "101");
    assert_eq!(choice.group.chat_id, "1");
    assert!(choice.member.is_none());
    assert!(service.take_order_choice("101", token).await.is_none());
    let (last, next) = alert_group_articles(&service, "101", &settings, groups.clone(), "", 20)
        .await
        .unwrap();
    assert_eq!(last.len(), 5);
    assert_eq!(next, "");
    let (found, _) = alert_group_articles(&service, "101", &settings, groups, "@GROUP25", 0)
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["title"], "Printing 25");
}

#[tokio::test]
async fn inline_member_results_show_selected_role_and_preserve_id_mentions() {
    let dir = tempfile::tempdir().unwrap();
    let service = TelegramService::new(dir.path().join("telegram.json"));
    let settings = TelegramAlertSettings {
        sender_user_id: Some("101".into()),
        group: Some(group(1, "Printing")),
        qolip_members: vec![member(7)],
        ..Default::default()
    };
    for (kind, label) in [
        (AlertKind::Qolip, "➖ Olib tashlash"),
        (AlertKind::RawMaterial, "➕ Qo‘shish"),
    ] {
        let article = alert_member_article(&service, "101", &settings, kind, member(7))
            .await
            .unwrap();
        let button = &article["reply_markup"]["inline_keyboard"][0][0];
        assert_eq!(button["text"], label);
        let token = button["callback_data"]
            .as_str()
            .unwrap()
            .strip_prefix("alert:toggle:")
            .unwrap();
        let choice: AlertChoice =
            serde_json::from_str(&service.take_order_choice("101", token).await.unwrap()).unwrap();
        assert_eq!(choice.kind, Some(kind));
        assert_eq!(choice.member.unwrap(), member(7));
        assert_eq!(choice.group, settings.group.clone().unwrap());
    }
}

#[tokio::test]
async fn unauthorized_inline_search_never_opens_a_profile_connection() {
    let dir = tempfile::tempdir().unwrap();
    let service = TelegramService::new(dir.path().join("telegram.json"));
    assert!(
        alert_inline_results(&service, "unknown", AlertInlineSearch::Groups(""), "")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn token_rotation_resets_offset_and_rejects_old_poller_updates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("telegram.json");
    let store = crate::telegram::store::TelegramStore::new(path.clone());
    store
        .set_bot_settings("old_bot".into(), Some("old-token".into()))
        .await
        .unwrap();
    store.set_update_offset("old-token", 999_999).await.unwrap();
    store
        .set_bot_settings("renamed_bot".into(), Some("old-token".into()))
        .await
        .unwrap();
    assert_eq!(store.polling_state().await.1, 999_999);
    store
        .set_bot_settings("new_bot".into(), Some("new-token".into()))
        .await
        .unwrap();
    assert_eq!(store.polling_state().await.1, 0);
    store
        .set_update_offset("old-token", 1_000_000)
        .await
        .unwrap();
    assert_eq!(store.polling_state().await.1, 0);
    store.set_update_offset("new-token", 5).await.unwrap();
    store.set_update_offset("new-token", 3).await.unwrap();
    assert_eq!(
        crate::telegram::store::TelegramStore::new(path)
            .polling_state()
            .await,
        ("new-token".into(), 5)
    );
}

#[tokio::test]
async fn a_slow_inline_search_does_not_serialize_another_search() {
    let permits = std::sync::Arc::new(tokio::sync::Semaphore::new(2));
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let first = tokio::spawn(limited_inline_search(permits.clone(), async {
        started_tx.send(()).unwrap();
        release_rx.await.unwrap();
        Ok(())
    }));
    started_rx.await.unwrap();
    let second = tokio::time::timeout(
        Duration::from_secs(1),
        limited_inline_search(permits, async { Ok(42) }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(second, 42);
    assert!(!first.is_finished());
    release_tx.send(()).unwrap();
    first.await.unwrap().unwrap();
}

#[tokio::test]
async fn inline_lookup_concurrency_is_bounded() {
    let permits = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let occupied = permits.clone().acquire_owned().await.unwrap();
    let (started_tx, mut started_rx) = tokio::sync::oneshot::channel();
    let pending = tokio::spawn(limited_inline_search(permits.clone(), async {
        started_tx.send(()).unwrap();
        Ok(())
    }));
    tokio::task::yield_now().await;
    assert_eq!(permits.available_permits(), 0);
    assert!(matches!(
        started_rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    drop(occupied);
    tokio::time::timeout(Duration::from_secs(1), started_rx)
        .await
        .unwrap()
        .unwrap();
    pending.await.unwrap().unwrap();
    assert_eq!(permits.available_permits(), 1);
}
