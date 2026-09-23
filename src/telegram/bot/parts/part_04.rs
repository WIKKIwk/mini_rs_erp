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
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
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
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
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
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
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

fn user_group_inline_keyboard() -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [
            [{
                "text": "🔎 Guruh qidirish",
                "switch_inline_query_current_chat": INLINE_GROUP_PREFIX
            }],
            [{"text": "↩️ Orqaga", "callback_data": "user_groups_back"}]
        ]
    })
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
    if service
        .order_draft(telegram_user_id)
        .await?
        .is_some_and(|draft| draft.pending_order_saved)
    {
        send_order_text(
            service,
            token,
            chat_id,
            "Oldingi order saqlangan, lekin hali guruhga yuborilmagan. Avval uni tasdiqlab yuboring.",
        )
        .await?;
        return Ok(());
    }
    clear_side_prompt(service, token, chat_id).await?;
    service.clear_order_attachment(telegram_user_id).await;
    service
        .save_order_draft(telegram_user_id, TelegramOrderDraft::default())
        .await?;
    send_customer_step(service, token, chat_id, telegram_user_id).await
}

async fn send_order_text(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    text: &str,
) -> Result<(), TelegramError> {
    send_or_edit_order_prompt(service, token, chat_id, text, None).await
}

async fn send_customer_step(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    telegram_user_id: &str,
) -> Result<(), TelegramError> {
    let _ = telegram_user_id;
    send_or_edit_order_prompt(
        service,
        token,
        chat_id,
        "👤 Mijozni tanlang:",
        Some(customer_step_keyboard()),
    )
    .await
}

async fn send_product_step(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    customer_name: &str,
    telegram_user_id: &str,
) -> Result<(), TelegramError> {
    let _ = telegram_user_id;
    send_or_edit_order_prompt(
        service,
        token,
        chat_id,
        &format!("✅ Mijoz: {customer_name}\n\n📦 Mahsulot nomini tanlang:"),
        Some(product_step_keyboard()),
    )
    .await
}

async fn send_status_step(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
) -> Result<(), TelegramError> {
    send_or_edit_order_prompt(
        service,
        token,
        chat_id,
        "📦 Buyurtma turini tanlang:",
        Some(status_keyboard()),
    )
    .await
}

async fn send_print_method_step(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
) -> Result<(), TelegramError> {
    send_or_edit_order_prompt(
        service,
        token,
        chat_id,
        "🖨 Bosma turini tanlang:",
        Some(print_method_keyboard()),
    )
    .await
}

async fn clear_side_prompt(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
) -> Result<(), TelegramError> {
    if let Some(mut draft) = service.order_draft(chat_id).await?
        && let Some(message_id) = draft.side_prompt_message_id.take()
    {
        delete_message(service, token, chat_id, message_id).await.ok();
        service.save_order_draft(chat_id, draft).await?;
    }
    Ok(())
}

async fn send_side_step(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
) -> Result<(), TelegramError> {
    // Keep the full order summary in its editable text message: photo captions
    // cannot hold long customer/product names and multiple material layers.
    send_order_text(service, token, chat_id, "Tarafini quyidagi rasm orqali tanlang.").await?;
    let Some(mut draft) = service.order_draft(chat_id).await? else {
        return Ok(());
    };
    if let Some(message_id) = draft.side_prompt_message_id {
        let response = request_json::<serde_json::Value, _>(
            service,
            token,
            "editMessageReplyMarkup",
            &serde_json::json!({
                "chat_id": chat_id,
                "message_id": message_id,
                "reply_markup": side_keyboard(),
            }),
        ).await;
        match response {
            Ok(_) => return Ok(()),
            Err(error) if error.to_string().contains("message is not modified") => return Ok(()),
            Err(_) => {
                delete_message(service, token, chat_id, message_id).await.ok();
            }
        }
    }
    let form = reqwest::multipart::Form::new()
        .text("chat_id", chat_id.to_string())
        .text("caption", "Tarafini tanlang:")
        .text("reply_markup", side_keyboard().to_string())
        .part(
            "photo",
            reqwest::multipart::Part::bytes(ORDER_SIDE_IMAGE.to_vec())
                .file_name("taraf.jpg")
                .mime_str("image/jpeg")
                .map_err(|error| TelegramError::Transport(error.to_string()))?,
        );
    let response = service
        .http_client()
        .post(bot_url(token, "sendPhoto"))
        .multipart(form)
        .send()
        .await
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
    let message: TelegramSentMessage = parse_api_response(response).await?;
    draft.side_prompt_message_id = Some(message.message_id);
    service.save_order_draft(chat_id, draft).await?;
    Ok(())
}

async fn send_order_review(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
) -> Result<(), TelegramError> {
    clear_side_prompt(service, token, chat_id).await?;
    send_or_edit_order_prompt(
        service,
        token,
        chat_id,
        "🧾 Buyurtmani tekshiring.\nTasdiqlash va yuborish yoki tahrirlash tugmasini bosing.",
        Some(order_review_keyboard()),
    )
    .await
}

async fn send_order_edit_menu(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
) -> Result<(), TelegramError> {
    send_or_edit_order_prompt(
        service,
        token,
        chat_id,
        "✏️ Qaysi bo‘limni tahrirlamoqchisiz?",
        Some(order_edit_keyboard()),
    )
    .await
}

async fn send_material_step(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    layer_number: usize,
) -> Result<(), TelegramError> {
    send_or_edit_order_prompt(
        service,
        token,
        chat_id,
        &format!("{layer_number}-qavat materialini tanlang:"),
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
    send_or_edit_order_prompt(
        service,
        token,
        chat_id,
        &format!(
            "✅ {}-qavat qo‘shildi. Yana qavat qo‘shasizmi?",
            draft.layers.len()
        ),
        Some(layer_options_keyboard()),
    )
    .await
}
