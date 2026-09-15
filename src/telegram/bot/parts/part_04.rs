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
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
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
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
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
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
    parse_api_response(response)
        .await
        .map(|_: serde_json::Value| ())
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
        .json(&body)
        .send()
        .await
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
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
        .json(&body)
        .send()
        .await
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
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
    let body = serde_json::json!({
        "inline_query_id": inline_query_id,
        "results": results,
        "cache_time": 0,
        "is_personal": true,
    });
    let response = service
        .http_client()
        .post(bot_url(token, "answerInlineQuery"))
        .json(&body)
        .send()
        .await
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
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
    (role == TelegramAccountRole::SalesManager).then(delivery_mode_keyboard)
}

fn account_guide_keyboard(account: &TelegramUserAccount) -> Option<serde_json::Value> {
    if !account.user_profile_connected {
        return role_guide_keyboard(account.role);
    }
    Some(serde_json::json!({
        "inline_keyboard": [[{
            "text": "👥 Guruh tanlash",
            "callback_data": "user_groups"
        }]]
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

fn user_group_keyboard(groups: &[TelegramUserGroup]) -> serde_json::Value {
    let rows = groups
        .iter()
        .take(20)
        .map(|group| {
            vec![serde_json::json!({
                "text": format!("{} · {}", group.title, group.chat_type),
                "callback_data": format!("user_group:{}:{}", group.chat_type, group.chat_id)
            })]
        })
        .collect::<Vec<_>>();
    serde_json::json!({"inline_keyboard": rows})
}

async fn start_new_order(
    service: &TelegramService,
    token: &str,
    telegram_user_id: &str,
    chat_id: &str,
) -> Result<(), TelegramError> {
    if service.order_catalog().await.is_err() {
        send_message(
            service,
            token,
            chat_id,
            "Order katalogi hali backendga ulanmagan.",
            None,
        )
        .await?;
        return Ok(());
    }
    service
        .save_order_draft(telegram_user_id, TelegramOrderDraft::default())
        .await?;
    send_customer_step(service, token, chat_id).await
}

async fn send_order_text(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    text: &str,
) -> Result<(), TelegramError> {
    send_message(service, token, chat_id, text, None).await
}

async fn send_customer_step(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
) -> Result<(), TelegramError> {
    send_message_with_markup(
        service,
        token,
        chat_id,
        "👤 Mijozni tanlang:",
        None,
        Some(customer_step_keyboard()),
    )
    .await
}

async fn send_product_step(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    customer_name: &str,
) -> Result<(), TelegramError> {
    send_message_with_markup(
        service,
        token,
        chat_id,
        &format!("✅ Mijoz: {customer_name}\n\n📦 Mahsulot nomini tanlang:"),
        None,
        Some(product_step_keyboard()),
    )
    .await
}

async fn send_status_step(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
) -> Result<(), TelegramError> {
    send_message_with_markup(
        service,
        token,
        chat_id,
        "Holatni tanlang:",
        None,
        Some(status_keyboard()),
    )
    .await
}

async fn send_material_step(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    layer_number: usize,
) -> Result<(), TelegramError> {
    send_message_with_markup(
        service,
        token,
        chat_id,
        &format!("{layer_number}-qavat materialini tanlang:"),
        None,
        Some(material_step_keyboard()),
    )
    .await
}

async fn send_micron_step(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
) -> Result<(), TelegramError> {
    send_order_text(
        service,
        token,
        chat_id,
        "Material tanlandi. Homashyo mikronini kiriting (musbat butun son):",
    )
    .await
}

async fn send_layer_options(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    draft: &TelegramOrderDraft,
) -> Result<(), TelegramError> {
    send_message_with_markup(
        service,
        token,
        chat_id,
        &format!(
            "✅ {}-qavat qo‘shildi. Yana qavat qo‘shasizmi?",
            draft.layers.len()
        ),
        None,
        Some(layer_options_keyboard()),
    )
    .await
}
