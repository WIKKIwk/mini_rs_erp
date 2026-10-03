pub(crate) async fn run_polling(service: TelegramService) {
    let mut active_token = String::new();
    let mut offset = 0;
    let searches = std::sync::Arc::new(tokio::sync::Semaphore::new(16));
    loop {
        let (token, saved_offset) = service.polling_state().await;
        if token != active_token {
            active_token = token.clone();
            offset = saved_offset;
        }
        if token.trim().is_empty() {
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        }
        match get_updates(&service, &token, offset).await {
            Ok(updates) => {
                if service.polling_state().await.0 != token {
                    continue;
                }
                for mut update in updates {
                    offset = offset.max(update.update_id.saturating_add(1));
                    if update.inline_query.as_ref().is_some_and(is_inline_search) {
                        // Searches do not change login/order state; slow MTProto lookups
                        // must not hold the polling loop or another user's commands.
                        spawn_inline_search(
                            service.clone(),
                            token.clone(),
                            update.inline_query.take().unwrap(),
                            searches.clone(),
                        );
                    } else if let Err(error) = handle_update(&service, &token, update).await {
                        tracing::warn!(?error, "telegram update handling failed");
                    }
                    if let Err(error) = service.set_update_offset(&token, offset).await {
                        tracing::warn!(?error, offset, "telegram update offset persist failed");
                    }
                }
            }
            Err(error) => {
                tracing::warn!(?error, "telegram polling failed");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

fn is_inline_search(query: &TelegramInlineQuery) -> bool {
    parse_alert_inline_query(&query.query).is_some()
        || parse_group_inline_query(&query.query).is_some()
        || parse_order_inline_query(&query.query).is_some()
}

async fn limited_inline_search<T>(
    permits: std::sync::Arc<tokio::sync::Semaphore>,
    work: impl std::future::Future<Output = Result<T, TelegramError>>,
) -> Result<T, TelegramError> {
    tokio::time::timeout(Duration::from_secs(8), async {
        let _permit = permits
            .acquire_owned()
            .await
            .map_err(|_| TelegramError::Store)?;
        work.await
    })
    .await
    .map_err(|_| TelegramError::Transport("inline search timed out".into()))?
}

fn spawn_inline_search(
    service: TelegramService,
    token: String,
    query: TelegramInlineQuery,
    permits: std::sync::Arc<tokio::sync::Semaphore>,
) {
    tokio::spawn(async move {
        let id = query.id.clone();
        if let Err(error) =
            limited_inline_search(permits, handle_inline_query(&service, &token, query)).await
        {
            tracing::warn!(?error, "telegram inline search failed");
            answer_inline_query(&service, &token, &id, vec![])
                .await
                .ok();
        }
    });
}
async fn get_updates(
    service: &TelegramService,
    token: &str,
    offset: i64,
) -> Result<Vec<TelegramUpdate>, TelegramError> {
    request_json(
        service,
        token,
        "getUpdates",
        &GetUpdatesRequest {
            offset,
            timeout: POLL_TIMEOUT_SECONDS,
            allowed_updates: ["message", "callback_query", "inline_query"],
        },
    )
    .await
}

async fn handle_update(
    service: &TelegramService,
    token: &str,
    update: TelegramUpdate,
) -> Result<(), TelegramError> {
    if let Some(inline_query) = update.inline_query {
        return handle_inline_query(service, token, inline_query).await;
    }
    if let Some(callback_query) = update.callback_query {
        return handle_callback_query(service, token, callback_query).await;
    }
    let Some(message) = update.message else {
        return Ok(());
    };
    let chat_id = message.chat.id.to_string();
    let is_private = message.chat.chat_type == "private";
    if is_private && message.from.is_some() && message.via_bot.is_none() {
        // Private chatdagi foydalanuvchi xabari keyingi bot prompti bilan
        // almashtiriladi; shu sabab tarixda qolmasligi uchun darhol tozalanadi.
        delete_message(service, token, &chat_id, message.message_id)
            .await
            .ok();
    }
    if message.via_bot.is_some() {
        if is_private {
            if message
                .text
                .as_deref()
                .is_some_and(|text| text.starts_with("Mijoz:"))
            {
                return handle_inline_customer_selection(service, token, &message).await;
            }
            if message
                .text
                .as_deref()
                .is_some_and(|text| text.starts_with("Mahsulot:"))
            {
                return handle_inline_product_selection(service, token, &message).await;
            }
            if message
                .text
                .as_deref()
                .is_some_and(|text| text.starts_with("Material:"))
            {
                return handle_inline_material_selection(service, token, &message).await;
            }
        }
        return Ok(());
    }
    if is_private && let Some(contact) = message.contact.as_ref() {
        return handle_contact(service, token, &message, contact).await;
    }
    if is_private && (message.photo.is_some() || message.document.is_some()) {
        return handle_private_media(service, token, &message).await;
    }
    let Some(text) = message.text.as_deref() else {
        return Ok(());
    };
    if is_private && handle_private_text(service, token, &message, text).await? {
        return Ok(());
    }
    let Some((command, argument)) = parse_command(text) else {
        return Ok(());
    };

    match command.as_str() {
        "start" if is_private => {
            let Some(user) = message.from.as_ref() else {
                return Ok(());
            };
            if argument.is_empty()
                && service
                    .user_by_telegram_id(&user.id.to_string())
                    .await?
                    .is_none()
            {
                send_message(
                    service,
                    token,
                    &chat_id,
                    "Assalomu alaykum! Accord botga xush kelibsiz. Admin yuborgan invite link orqali qayta Start bosing.",
                    None,
                )
                .await?;
                return Ok(());
            }
            let account = service
                .register_bot_start(TelegramStartRequest {
                    invite_token: argument,
                    telegram_user_id: user.id.to_string(),
                    telegram_chat_id: chat_id.clone(),
                    username: user.username.clone().unwrap_or_default(),
                    display_name: telegram_display_name(user),
                })
                .await;
            match account {
                Ok(account) => {
                    let text = account_guide(&account);
                    send_message_with_markup(
                        service,
                        token,
                        &chat_id,
                        &text,
                        None,
                        account_guide_keyboard(&account),
                    )
                    .await?;
                }
                Err(error) => {
                    let text = format!("Ulanish amalga oshmadi: {}", start_error_message(&error));
                    send_message(service, token, &chat_id, &text, None).await?;
                }
            }
        }
        "login" if is_private => {
            if let Some(user) = message.from.as_ref() {
                begin_bot_profile_login(service, token, &chat_id, &user.id.to_string()).await?;
            }
        }
        "help" | "commands" => {
            let text = match message.from.as_ref() {
                Some(user) => match service.user_by_telegram_id(&user.id.to_string()).await? {
                    Some(account) => {
                        let text = if is_private {
                            account_guide(&account)
                        } else {
                            role_guide(account.role)
                        };
                        send_message_with_markup(
                            service,
                            token,
                            &chat_id,
                            &text,
                            message.message_thread_id,
                            if is_private {
                                account_guide_keyboard(&account)
                            } else {
                                None
                            },
                        )
                        .await?;
                        return Ok(());
                    }
                    None => general_guide().to_string(),
                },
                None => general_guide().to_string(),
            };
            send_message(service, token, &chat_id, &text, message.message_thread_id).await?;
        }
        "start" if !is_private && argument == "connect" => {
            connect_group(service, token, &message).await?;
        }
        "connect" if !is_private => {
            connect_group(service, token, &message).await?;
        }
        _ => {}
    }
    Ok(())
}
