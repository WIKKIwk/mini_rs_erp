pub(crate) async fn send_order_to_chat(
    service: &TelegramService,
    chat: &TelegramChat,
    notification: &TelegramOrderNotification,
) -> Result<(), TelegramError> {
    let (_, token) = service.bot_credentials().await?;
    if token.trim().is_empty() {
        return Err(TelegramError::BotTokenRequired);
    }
    if let Some(image) = notification.image.as_ref() {
        if can_send_as_telegram_photo(image) {
            send_photo(service, &token, chat, notification, image).await
        } else {
            send_document(service, &token, chat, notification, image).await
        }
    } else {
        send_message(
            service,
            &token,
            &chat.chat_id,
            &notification.caption,
            chat.thread_id,
        )
        .await
    }
}

async fn send_photo(
    service: &TelegramService,
    token: &str,
    chat: &TelegramChat,
    notification: &TelegramOrderNotification,
    image: &CalculateOrderImage,
) -> Result<(), TelegramError> {
    let file_name = telegram_file_name(image);
    let mut form = reqwest::multipart::Form::new()
        .text("chat_id", chat.chat_id.clone())
        .text("caption", truncate_caption(&notification.caption))
        .part(
            "photo",
            reqwest::multipart::Part::bytes(image.body.clone()).file_name(file_name),
        );
    if let Some(thread_id) = chat.thread_id {
        form = form.text("message_thread_id", thread_id.to_string());
    }
    let response = service
        .http_client()
        .post(bot_url(token, "sendPhoto"))
        .multipart(form)
        .send()
        .await
        .map_err(bot_transport_error)?;
    parse_api_response(response)
        .await
        .map(|_: serde_json::Value| ())
}

async fn send_document(
    service: &TelegramService,
    token: &str,
    chat: &TelegramChat,
    notification: &TelegramOrderNotification,
    image: &CalculateOrderImage,
) -> Result<(), TelegramError> {
    let file_name = telegram_file_name(image);
    let mut form = reqwest::multipart::Form::new()
        .text("chat_id", chat.chat_id.clone())
        .text("caption", truncate_caption(&notification.caption))
        .part(
            "document",
            reqwest::multipart::Part::bytes(image.body.clone()).file_name(file_name),
        );
    if let Some(thread_id) = chat.thread_id {
        form = form.text("message_thread_id", thread_id.to_string());
    }
    let response = service
        .http_client()
        .post(bot_url(token, "sendDocument"))
        .multipart(form)
        .send()
        .await
        .map_err(bot_transport_error)?;
    parse_api_response(response)
        .await
        .map(|_: serde_json::Value| ())
}

fn can_send_as_telegram_photo(image: &CalculateOrderImage) -> bool {
    matches!(
        image.image_mime.trim().to_ascii_lowercase().as_str(),
        "image/jpeg" | "image/jpg" | "image/png"
    )
}

fn telegram_file_name(image: &CalculateOrderImage) -> String {
    if image.image_name.trim().is_empty() {
        "order-image.jpg".to_string()
    } else {
        image.image_name.trim().to_string()
    }
}

async fn send_message(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    text: &str,
    thread_id: Option<i64>,
) -> Result<(), TelegramError> {
    send_message_with_markup(service, token, chat_id, text, thread_id, None).await
}

async fn send_message_with_markup(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    text: &str,
    thread_id: Option<i64>,
    reply_markup: Option<serde_json::Value>,
) -> Result<(), TelegramError> {
    send_message_with_markup_result(service, token, chat_id, text, thread_id, reply_markup)
        .await
        .map(|_| ())
}

async fn send_message_with_markup_result(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    text: &str,
    thread_id: Option<i64>,
    reply_markup: Option<serde_json::Value>,
) -> Result<i64, TelegramError> {
    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
    });
    if let Some(thread_id) = thread_id {
        body["message_thread_id"] = serde_json::json!(thread_id);
    }
    if let Some(reply_markup) = reply_markup {
        body["reply_markup"] = reply_markup;
    }
    let response = service
        .http_client()
        .post(bot_url(token, "sendMessage"))
        .json(&body)
        .send()
        .await
        .map_err(bot_transport_error)?;
    let message: TelegramSentMessage = parse_api_response(response).await?;
    Ok(message.message_id)
}

async fn send_or_edit_order_prompt(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    text: &str,
    reply_markup: Option<serde_json::Value>,
) -> Result<(), TelegramError> {
    let draft = service.order_draft(chat_id).await?;
    let text = if let Some(draft) = draft.as_ref() {
        let has_image = draft.pending_order_saved || service.order_attachment(chat_id).await.is_some();
        order_prompt(&draft.order_number, draft, has_image, text)
    } else {
        text.to_string()
    };
    let edit_markup = reply_markup
        .clone()
        .or_else(|| Some(serde_json::json!({"inline_keyboard": []})));
    if let Some(mut draft) = draft {
        if let Some(inline_message_id) = draft.prompt_inline_message_id.clone() {
            match edit_inline_message_with_markup(
                service,
                token,
                &inline_message_id,
                &text,
                edit_markup.clone(),
            )
            .await
            {
                Ok(()) => return Ok(()),
                Err(error) if error.to_string().contains("message is not modified") => {
                    return Ok(())
                }
                Err(_) => {
                    draft.prompt_inline_message_id = None;
                    service.save_order_draft(chat_id, draft.clone()).await?;
                }
            }
        }
        if let Some(message_id) = draft.prompt_message_id {
            match edit_message_with_markup(
                service,
                token,
                chat_id,
                message_id,
                &text,
                edit_markup,
            )
            .await
            {
                Ok(()) => return Ok(()),
                Err(error) if error.to_string().contains("message is not modified") => {
                    return Ok(())
                }
                Err(_) => {
                    delete_message(service, token, chat_id, message_id)
                        .await
                        .ok();
                }
            }
        }
    }
    let message_id = send_message_with_markup_result(
        service,
        token,
        chat_id,
        &text,
        None,
        reply_markup,
    )
    .await?;
    if let Some(mut draft) = service.order_draft(chat_id).await? {
        draft.prompt_message_id = Some(message_id);
        service.save_order_draft(chat_id, draft).await?;
    }
    Ok(())
}

async fn edit_message_with_markup(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    message_id: i64,
    text: &str,
    reply_markup: Option<serde_json::Value>,
) -> Result<(), TelegramError> {
    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
        "text": text,
    });
    if let Some(reply_markup) = reply_markup {
        body["reply_markup"] = reply_markup;
    }
    let response = service
        .http_client()
        .post(bot_url(token, "editMessageText"))
        .json(&body)
        .send()
        .await
        .map_err(bot_transport_error)?;
    parse_api_response(response)
        .await
        .map(|_: serde_json::Value| ())
}

async fn edit_inline_message_with_markup(
    service: &TelegramService,
    token: &str,
    inline_message_id: &str,
    text: &str,
    reply_markup: Option<serde_json::Value>,
) -> Result<(), TelegramError> {
    let mut body = serde_json::json!({
        "inline_message_id": inline_message_id,
        "text": text,
    });
    if let Some(reply_markup) = reply_markup {
        body["reply_markup"] = reply_markup;
    }
    let response = service
        .http_client()
        .post(bot_url(token, "editMessageText"))
        .json(&body)
        .send()
        .await
        .map_err(bot_transport_error)?;
    parse_api_response(response)
        .await
        .map(|_: bool| ())
}

async fn delete_message(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    message_id: i64,
) -> Result<(), TelegramError> {
    // Login kodi oddiy xabar bo'lib tarixda qolmasligi uchun. Best-effort:
    // botda huquq bo'lmasa xatolik chaqiruvchida e'tiborsiz qoldiriladi.
    if message_id <= 0 {
        return Ok(());
    }
    let body = serde_json::json!({
        "chat_id": chat_id,
        "message_id": message_id,
    });
    let response = service
        .http_client()
        .post(bot_url(token, "deleteMessage"))
        .timeout(Duration::from_secs(2))
        .json(&body)
        .send()
        .await
        .map_err(bot_transport_error)?;
    parse_api_response(response).await.map(|_: bool| ())
}

async fn answer_callback_query(
    service: &TelegramService,
    token: &str,
    callback_query_id: &str,
    text: Option<&str>,
    show_alert: bool,
) -> Result<(), TelegramError> {
    let mut body = serde_json::json!({
        "callback_query_id": callback_query_id,
        "show_alert": show_alert,
    });
    if let Some(text) = text {
        body["text"] = serde_json::json!(text);
    }
    let response = service
        .http_client()
        .post(bot_url(token, "answerCallbackQuery"))
        .timeout(Duration::from_secs(2))
        .json(&body)
        .send()
        .await
        .map_err(bot_transport_error)?;
    parse_api_response(response)
        .await
        .map(|_: serde_json::Value| ())
}

async fn answer_inline_query(
    service: &TelegramService,
    token: &str,
    inline_query_id: &str,
    results: Vec<serde_json::Value>,
) -> Result<(), TelegramError> {
    answer_inline_query_page(service, token, inline_query_id, results, "").await
}

async fn answer_inline_query_page(
    service: &TelegramService,
    token: &str,
    inline_query_id: &str,
    results: Vec<serde_json::Value>,
    next_offset: &str,
) -> Result<(), TelegramError> {
    let body = serde_json::json!({
        "inline_query_id": inline_query_id,
        "results": results,
        "cache_time": 0,
        "is_personal": true,
        "next_offset": next_offset,
    });
    let response = service
        .http_client()
        .post(bot_url(token, "answerInlineQuery"))
        .timeout(Duration::from_secs(3))
        .json(&body)
        .send()
        .await
        .map_err(bot_transport_error)?;
    parse_api_response(response)
        .await
        .map(|_: serde_json::Value| ())
}

async fn send_inline_login_prompt(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    text: &str,
    query_prefix: &str,
    button_text: &str,
) -> Result<(), TelegramError> {
    send_message_with_markup(
        service,
        token,
        chat_id,
        text,
        None,
        Some(login_inline_keyboard(query_prefix, button_text)),
    )
    .await
}
fn role_guide_keyboard(role: TelegramAccountRole) -> Option<serde_json::Value> {
    if role == TelegramAccountRole::AlertSender {
        return Some(serde_json::json!({"inline_keyboard": [[{
            "text": "👤 Telegram profilini ulash", "callback_data": "login:start"
        }]]}));
    }
    (role == TelegramAccountRole::SalesManager).then(delivery_mode_keyboard)
}

fn account_guide_keyboard(account: &TelegramUserAccount) -> Option<serde_json::Value> {
    if !account.user_profile_connected {
        return role_guide_keyboard(account.role);
    }
    if account.role == TelegramAccountRole::AlertSender {
        return Some(serde_json::json!({"inline_keyboard": [[{
            "text": "🔔 Guruh va mas’ullarni tanlash", "callback_data": "alert:home"
        }]]}));
    }
    Some(serde_json::json!({
        "inline_keyboard": [[{
            "text": "👥 Guruh tanlash",
            "callback_data": "user_groups"
        }], [{"text": "🔔 Ogohlantirishlar", "callback_data": "alert:home"}]]
    }))
}

fn delivery_mode_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [{"text": "🤖 Bot orqali", "callback_data": "delivery:bot"}],
            [{"text": "👤 User profile orqali", "callback_data": "delivery:user"}]
        ]
    })
}

fn login_inline_keyboard(query_prefix: &str, button_text: &str) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [[{
            "text": button_text,
            "switch_inline_query_current_chat": query_prefix
        }]]
    })
}

fn code_sent_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [{
                "text": "🔐 Kodni inline yuborish",
                "switch_inline_query_current_chat": INLINE_CODE_PREFIX
            }],
            [{
                "text": "📨 Kod kelmadi — qayta yuborish",
                "callback_data": "login:resend"
            }]
        ]
    })
}

async fn send_code_sent_prompt(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    text: &str,
) -> Result<(), TelegramError> {
    send_message_with_markup(service, token, chat_id, text, None, Some(code_sent_keyboard())).await
}

fn contact_request_markup() -> serde_json::Value {
    serde_json::json!({
        "keyboard": [[{
            "text": "📱 Telefon raqamini yuborish",
            "request_contact": true
        }]],
        "resize_keyboard": true,
        "one_time_keyboard": true
    })
}

fn remove_keyboard_markup() -> serde_json::Value {
    serde_json::json!({"remove_keyboard": true})
}

include!("user_group_inline_keyboard.rs");
