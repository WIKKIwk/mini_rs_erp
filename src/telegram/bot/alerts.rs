use crate::telegram::alerts::AlertKind;
use crate::telegram::alerts::models::AlertMember;

#[derive(serde::Serialize, serde::Deserialize)]
struct AlertChoice {
    sender: String,
    group: TelegramUserGroup,
    member: Option<AlertMember>,
    kind: Option<AlertKind>,
    page: usize,
}

fn alert_callback_is_allowed(callback: &TelegramCallbackQuery) -> bool {
    callback
        .message
        .as_ref()
        .is_some_and(|m| m.chat.chat_type == "private")
        || (callback.message.is_none()
            && callback.inline_message_id.is_some()
            && callback.data.as_deref().is_some_and(|data| {
                data.starts_with("alert:group:") || data.starts_with("alert:toggle:")
            }))
}

async fn begin_bot_profile_login(
    service: &TelegramService,
    token: &str,
    chat: &str,
    actor: &str,
) -> Result<(), TelegramError> {
    let Some(account) = service.user_by_telegram_id(actor).await? else {
        return send_message(service, token, chat,
            "Avval admin mobile ilovadan yuborgan Ogohlantiruvchi taklif linkini ochib, Start bosing.", None).await;
    };
    if account.user_profile_connected {
        send_login_destination(service, token, chat, actor).await
    } else {
        send_contact_request(service, token, chat).await
    }
}

async fn send_login_destination(
    service: &TelegramService,
    token: &str,
    chat: &str,
    actor: &str,
) -> Result<(), TelegramError> {
    let account = service
        .user_by_telegram_id(actor)
        .await?
        .ok_or(TelegramError::UserAccountNotAuthorized)?;
    if account.role == TelegramAccountRole::AlertSender {
        show_alert_panel(service, token, chat, actor, None).await
    } else {
        send_user_group_picker(service, token, chat).await
    }
}

async fn alert_reply(
    service: &TelegramService,
    token: &str,
    chat: &str,
    message_id: Option<i64>,
    text: &str,
    rows: Vec<Vec<serde_json::Value>>,
) -> Result<(), TelegramError> {
    let markup = Some(serde_json::json!({"inline_keyboard": rows}));
    if let Some(id) = message_id {
        edit_message_with_markup(service, token, chat, id, text, markup).await
    } else {
        send_message_with_markup(service, token, chat, text, None, markup).await
    }
}

fn alert_button(text: &str, callback: &str) -> serde_json::Value {
    serde_json::json!({"text":text,"callback_data":callback})
}

async fn show_alert_panel(
    service: &TelegramService,
    token: &str,
    chat: &str,
    actor: &str,
    message_id: Option<i64>,
) -> Result<(), TelegramError> {
    if service.authorize_alert_configuration(actor).await.is_err() {
        return alert_reply(
            service,
            token,
            chat,
            message_id,
            "Ogohlantirishni faqat admin yoki mobile’da tanlangan ogohlantiruvchi profil sozlaydi.",
            vec![],
        )
        .await;
    }
    let settings = service.store.alert_settings().await;
    let sender = match settings.sender_user_id.as_deref() {
        Some(id) => service.user_by_telegram_id(id).await?.map(|u| u.display_name).unwrap_or_default(),
        None => return alert_reply(service, token, chat, message_id,
            "Avval mobile ilovadagi Telegram bo‘limida ulangan profilni «Ogohlantiruvchi» qilib tanlang.", vec![]).await,
    };
    let list = |kind: AlertKind| {
        let names: Vec<_> = settings
            .members(kind)
            .iter()
            .take(8)
            .map(|m| {
                if m.username.is_empty() {
                    m.display_name.clone()
                } else {
                    format!("@{}", m.username)
                }
            })
            .map(|name| name.chars().take(64).collect::<String>())
            .collect();
        if names.is_empty() {
            "Tanlanmagan".into()
        } else {
            let remaining = settings.members(kind).len().saturating_sub(names.len());
            let mut text = names.join(", ");
            if remaining > 0 {
                text.push_str(&format!(" (+{remaining})"));
            }
            text
        }
    };
    let group = settings
        .group
        .as_ref()
        .map(|g| g.title.as_str())
        .unwrap_or("Tanlanmagan");
    let text = format!(
        "🔔 Ishlab chiqarish ogohlantirishlari\nYuboruvchi: {sender}\nGuruh: {group}\n\nMaterial ta’minotchi: {}\nQolipchi: {}\n\nBosmachi tugmani bosganda shu guruhdagi tegishli mas’ullar atmetka qilinadi.",
        list(AlertKind::RawMaterial),
        list(AlertKind::Qolip)
    );
    let mut rows = vec![vec![alert_search_button(
        "🔎 Guruh tanlash",
        INLINE_ALERT_GROUP_PREFIX,
    )]];
    if settings.group.is_some() {
        rows.push(vec![alert_search_button(
            "🔎 Material ta’minotchilar",
            INLINE_ALERT_MATERIAL_PREFIX,
        )]);
        rows.push(vec![alert_search_button(
            "🔎 Qolipchilar",
            INLINE_ALERT_QOLIP_PREFIX,
        )]);
    }
    alert_reply(service, token, chat, message_id, &text, rows).await?;
    warm_alert_lookup(service, &settings);
    Ok(())
}

fn warm_alert_lookup(
    service: &TelegramService,
    settings: &crate::telegram::alerts::TelegramAlertSettings,
) {
    if cfg!(test) {
        return;
    }
    let Some(sender) = settings.sender_user_id.clone() else {
        return;
    };
    let service = service.clone();
    let group = settings.group.clone();
    tokio::spawn(async move {
        let _ = tokio::time::timeout(Duration::from_secs(30), async {
            service.useraccount.writable_groups(&sender).await?;
            if let Some(group) = group {
                service
                    .useraccount
                    .alert_member_search(&sender, &group, "", 0)
                    .await?;
            }
            Ok::<_, super::useraccount::UserAccountError>(())
        })
        .await;
    });
}

async fn handle_alert_callback(
    service: &TelegramService,
    token: &str,
    chat: &str,
    actor: &str,
    data: &str,
    message_id: Option<i64>,
) -> Result<(), TelegramError> {
    if service.authorize_alert_configuration(actor).await.is_err() {
        return show_alert_panel(service, token, chat, actor, message_id).await;
    }
    let result = async {
        if data == "alert:home" {
            return show_alert_panel(service, token, chat, actor, message_id).await;
        }
        if let Some(page) = data
            .strip_prefix("alert:groups:")
            .and_then(|p| p.parse::<usize>().ok())
            .filter(|p| *p <= 10_000)
        {
            return show_alert_groups(service, token, chat, actor, message_id, page).await;
        }
        if let Some(value) = data.strip_prefix("alert:members:") {
            if let Some((kind, page)) = value.split_once(':') {
                if let (Some(kind), Ok(page)) = (AlertKind::parse(kind), page.parse::<usize>()) {
                    return show_alert_members(service, token, chat, actor, message_id, kind, page)
                        .await;
                }
            }
        }
        let Some((action, choice_token)) =
            data.strip_prefix("alert:").and_then(|s| s.split_once(':'))
        else {
            return Ok(());
        };
        if action != "group" && action != "toggle" {
            return Ok(());
        }
        let choice = service
            .take_order_choice(actor, choice_token)
            .await
            .ok_or_else(|| {
                TelegramError::UserAccount("Tanlov eskirgan. /alerts ni qayta oching".into())
            })?;
        let choice: AlertChoice =
            serde_json::from_str(&choice).map_err(|_| TelegramError::Store)?;
        let settings = service.store.alert_settings().await;
        if settings.sender_user_id.as_deref() != Some(&choice.sender) {
            return show_alert_panel(service, token, chat, actor, message_id).await;
        }
        if action == "group" {
            let group = service
                .useraccount
                .writable_groups(&choice.sender)
                .await
                .map_err(super::service::map_user_account)?
                .into_iter()
                .find(|g| {
                    g.chat_id == choice.group.chat_id && g.chat_type == choice.group.chat_type
                })
                .ok_or(TelegramError::UserAccountGroupNotWritable)?;
            service
                .store
                .set_alert_group(&choice.sender, group)
                .await
                .map_err(|_| TelegramError::Store)?;
            return show_alert_panel(service, token, chat, actor, message_id).await;
        }
        if settings.group.as_ref() != Some(&choice.group) {
            return show_alert_panel(service, token, chat, actor, message_id).await;
        }
        let (Some(member), Some(kind)) = (choice.member, choice.kind) else {
            return Ok(());
        };
        // The actor-bound token comes from Telegram's actual group-member search.
        service
            .store
            .toggle_alert_member(&settings, kind, member)
            .await
            .map_err(|_| TelegramError::Store)?;
        show_alert_members(service, token, chat, actor, message_id, kind, choice.page).await
    }
    .await;
    if let Err(error) = result {
        tracing::warn!(%error, "Telegram alert configuration failed");
        alert_reply(service, token, chat, message_id,
            "Sozlama bajarilmadi. Profil guruhga yozishi va guruh a’zolarini ko‘ra olishi kerak. /alerts ni qayta ochib urinib ko‘ring (har rolga 30 tagacha mas’ul).",
            vec![vec![alert_button("Orqaga", "alert:home")]]).await?;
    }
    Ok(())
}
