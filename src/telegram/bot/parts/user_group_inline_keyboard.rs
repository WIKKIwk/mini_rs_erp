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
                .map_err(bot_transport_error)?,
        );
    let response = service
        .http_client()
        .post(bot_url(token, "sendPhoto"))
        .multipart(form)
        .send()
        .await
        .map_err(bot_transport_error)?;
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
