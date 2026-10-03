const INLINE_ALERT_GROUP_PREFIX: &str = "ag7 ";
const INLINE_ALERT_MATERIAL_PREFIX: &str = "am7 ";
const INLINE_ALERT_QOLIP_PREFIX: &str = "aq7 ";
const ALERT_INLINE_PAGE_SIZE: usize = 20;

#[derive(Debug, PartialEq)]
enum AlertInlineSearch<'a> {
    Groups(&'a str),
    Members(AlertKind, &'a str),
}

fn parse_alert_inline_query(query: &str) -> Option<AlertInlineSearch<'_>> {
    let query = query.trim_start();
    for (prefix, kind) in [
        (INLINE_ALERT_GROUP_PREFIX, None),
        (INLINE_ALERT_MATERIAL_PREFIX, Some(AlertKind::RawMaterial)),
        (INLINE_ALERT_QOLIP_PREFIX, Some(AlertKind::Qolip)),
    ] {
        let value = if query == prefix.trim_end() {
            ""
        } else if let Some(value) = query.strip_prefix(prefix) {
            value.trim()
        } else {
            continue;
        };
        return Some(match kind {
            Some(kind) => AlertInlineSearch::Members(kind, value),
            None => AlertInlineSearch::Groups(value),
        });
    }
    None
}

fn alert_search_button(text: &str, prefix: &str) -> serde_json::Value {
    serde_json::json!({"text": text, "switch_inline_query_current_chat": prefix})
}

fn alert_search_rows(kind: Option<AlertKind>) -> Vec<Vec<serde_json::Value>> {
    let (label, prefix) = match kind {
        None => ("🔎 Guruhlarni qidirish", INLINE_ALERT_GROUP_PREFIX),
        Some(AlertKind::RawMaterial) => (
            "🔎 Material ta’minotchini qidirish",
            INLINE_ALERT_MATERIAL_PREFIX,
        ),
        Some(AlertKind::Qolip) => ("🔎 Qolipchini qidirish", INLINE_ALERT_QOLIP_PREFIX),
    };
    vec![
        vec![alert_search_button(label, prefix)],
        vec![alert_button("Tayyor / Orqaga", "alert:home")],
    ]
}

async fn alert_choice_token(
    service: &TelegramService,
    actor: &str,
    choice: AlertChoice,
) -> Result<String, TelegramError> {
    let value = serde_json::to_string(&choice).map_err(|_| TelegramError::Store)?;
    Ok(service.remember_order_choice(actor, value).await)
}

// Old messages keep working, but now open the inline picker without fetching a roster.
async fn show_alert_groups(
    service: &TelegramService,
    token: &str,
    chat: &str,
    _actor: &str,
    message_id: Option<i64>,
    _page: usize,
) -> Result<(), TelegramError> {
    alert_reply(service, token, chat, message_id,
        "Guruhni inline qidiruvdan tanlang. Nom yoki @username yozib qidiring, natijani yuboring va «Tanlash»ni bosing. Guruh almashtirilsa, mas’ullarni qayta tanlaysiz.",
        alert_search_rows(None)).await
}

async fn show_alert_members(
    service: &TelegramService,
    token: &str,
    chat: &str,
    actor: &str,
    message_id: Option<i64>,
    kind: AlertKind,
    _page: usize,
) -> Result<(), TelegramError> {
    let settings = service.store.alert_settings().await;
    let Some(group) = settings.group.as_ref() else {
        return show_alert_panel(service, token, chat, actor, message_id).await;
    };
    let text = format!(
        "{} · {}\nTanlanganlar: {}\n\nIsm yoki @username bo‘yicha inline qidiring. Natijani yuborib, «Qo‘shish» yoki «Olib tashlash»ni bosing.",
        group.title,
        kind.label(),
        settings.members(kind).len()
    );
    alert_reply(
        service,
        token,
        chat,
        message_id,
        &text,
        alert_search_rows(Some(kind)),
    )
    .await
}

async fn alert_inline_results(
    service: &TelegramService,
    actor: &str,
    search: AlertInlineSearch<'_>,
    offset: &str,
) -> Result<(Vec<serde_json::Value>, String), TelegramError> {
    service.authorize_alert_configuration(actor).await?;
    let settings = service.store.alert_settings().await;
    let Some(sender) = settings.sender_user_id.as_deref() else {
        return Ok((vec![], String::new()));
    };
    let offset = if offset.is_empty() {
        0
    } else {
        match offset.parse::<usize>() {
            Ok(offset) if offset <= 100_000 => offset,
            _ => return Ok((vec![], String::new())),
        }
    };
    match search {
        AlertInlineSearch::Groups(query) => {
            let groups = service
                .useraccount
                .writable_groups(sender)
                .await
                .map_err(super::service::map_user_account)?;
            alert_group_articles(service, actor, &settings, groups, query, offset).await
        }
        AlertInlineSearch::Members(kind, query) => {
            let Some(group) = settings.group.as_ref() else {
                return Ok((vec![], String::new()));
            };
            let page = service
                .useraccount
                .alert_member_search(sender, group, query, offset)
                .await
                .map_err(super::service::map_user_account)?;
            let next = if page.has_more {
                (offset + ALERT_INLINE_PAGE_SIZE).to_string()
            } else {
                String::new()
            };
            let mut articles = vec![];
            for member in page.members {
                articles.push(alert_member_article(service, actor, &settings, kind, member).await?);
            }
            Ok((articles, next))
        }
    }
}

async fn alert_group_articles(
    service: &TelegramService,
    actor: &str,
    settings: &crate::telegram::alerts::TelegramAlertSettings,
    groups: Vec<TelegramUserGroup>,
    query: &str,
    offset: usize,
) -> Result<(Vec<serde_json::Value>, String), TelegramError> {
    let Some(sender) = settings.sender_user_id.as_ref() else {
        return Ok((vec![], String::new()));
    };
    let query = query.trim().trim_start_matches('@').to_lowercase();
    let mut groups: Vec<_> = groups
        .into_iter()
        .filter(|g| {
            query.is_empty()
                || g.title.to_lowercase().contains(&query)
                || g.username.to_lowercase().contains(&query)
        })
        .collect();
    groups.sort_by_key(|g| g.title.to_lowercase());
    let next = if groups.len() > offset.saturating_add(ALERT_INLINE_PAGE_SIZE) {
        (offset + ALERT_INLINE_PAGE_SIZE).to_string()
    } else {
        String::new()
    };
    let mut articles = vec![];
    for group in groups.into_iter().skip(offset).take(ALERT_INLINE_PAGE_SIZE) {
        let selected = settings
            .group
            .as_ref()
            .is_some_and(|g| g.chat_id == group.chat_id && g.chat_type == group.chat_type);
        let title = format!("{}{}", if selected { "✅ " } else { "" }, group.title);
        let description = if group.username.is_empty() {
            group.chat_type.clone()
        } else {
            format!("@{}", group.username)
        };
        let text = format!("Ogohlantirish guruhi: {}", group.title);
        let choice = alert_choice_token(
            service,
            actor,
            AlertChoice {
                sender: sender.clone(),
                group,
                member: None,
                kind: None,
                page: 0,
            },
        )
        .await?;
        articles.push(inline_article(
            &choice,
            &title,
            &description,
            &text,
            &format!("alert:group:{choice}"),
        ));
    }
    Ok((articles, next))
}

async fn alert_member_article(
    service: &TelegramService,
    actor: &str,
    settings: &crate::telegram::alerts::TelegramAlertSettings,
    kind: AlertKind,
    member: AlertMember,
) -> Result<serde_json::Value, TelegramError> {
    let selected = settings
        .members(kind)
        .iter()
        .any(|m| m.user_id == member.user_id);
    let name = if member.display_name.is_empty() {
        member.user_id.to_string()
    } else {
        member.display_name.clone()
    };
    let title = format!("{}{}", if selected { "✅ " } else { "" }, name);
    let description = format!(
        "{} · {}",
        kind.label(),
        if member.username.is_empty() {
            member.user_id.to_string()
        } else {
            format!("@{}", member.username)
        }
    );
    let text = format!("{}: {name}\n{}", kind.label(), description);
    let choice = alert_choice_token(
        service,
        actor,
        AlertChoice {
            sender: settings
                .sender_user_id
                .clone()
                .ok_or(TelegramError::UserAccountNotAuthorized)?,
            group: settings
                .group
                .clone()
                .ok_or(TelegramError::UserAccountGroupNotWritable)?,
            member: Some(member),
            kind: Some(kind),
            page: 0,
        },
    )
    .await?;
    let mut article = inline_article(
        &choice,
        &title,
        &description,
        &text,
        &format!("alert:toggle:{choice}"),
    );
    article["reply_markup"]["inline_keyboard"][0][0]["text"] = serde_json::json!(if selected {
        "➖ Olib tashlash"
    } else {
        "➕ Qo‘shish"
    });
    Ok(article)
}
