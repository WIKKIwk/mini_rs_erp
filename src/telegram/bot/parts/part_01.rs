pub(crate) async fn run_polling(service: TelegramService) {
    let mut offset = service.update_offset().await.unwrap_or_default();
    loop {
        let (_, token) = match service.bot_credentials().await {
            Ok(credentials) => credentials,
            Err(error) => {
                tracing::warn!(?error, "telegram bot settings unavailable");
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
        };
        if token.trim().is_empty() {
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        }
        match get_updates(&service, &token, offset).await {
            Ok(updates) => {
                for update in updates {
                    offset = offset.max(update.update_id.saturating_add(1));
                    if let Err(error) = handle_update(&service, &token, update).await {
                        tracing::warn!(?error, "telegram update handling failed");
                    }
                    if let Err(error) = service.set_update_offset(offset).await {
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
            if argument.is_empty() {
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
                .register_start(TelegramStartRequest {
                    invite_token: argument,
                    telegram_user_id: user.id.to_string(),
                    telegram_chat_id: chat_id.clone(),
                    username: user.username.clone().unwrap_or_default(),
                    display_name: telegram_display_name(user),
                })
                .await;
            match account {
                Ok(account) => {
                    let text = role_guide(account.role);
                    send_message_with_markup(
                        service,
                        token,
                        &chat_id,
                        &text,
                        None,
                        role_guide_keyboard(account.role),
                    )
                    .await?;
                }
                Err(error) => {
                    let text = format!("Ulanish amalga oshmadi: {}", start_error_message(&error));
                    send_message(service, token, &chat_id, &text, None).await?;
                }
            }
        }
        "help" | "commands" => {
            let text = match message.from.as_ref() {
                Some(user) => match service.user_by_telegram_id(&user.id.to_string()).await? {
                    Some(account) => {
                        let text = role_guide(account.role);
                        send_message_with_markup(
                            service,
                            token,
                            &chat_id,
                            &text,
                            message.message_thread_id,
                            if is_private {
                                role_guide_keyboard(account.role)
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
