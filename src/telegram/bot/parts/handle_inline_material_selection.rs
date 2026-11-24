async fn handle_inline_material_selection(
    service: &TelegramService,
    token: &str,
    message: &TelegramMessage,
) -> Result<(), TelegramError> {
    let Some(user) = message.from.as_ref() else {
        return Ok(());
    };
    let telegram_user_id = user.id.to_string();
    let Some(mut draft) = service.order_draft(&telegram_user_id).await? else {
        return Ok(());
    };
    if draft.step != TelegramOrderStep::Material {
        return Ok(());
    }
    let Some(material_name) = message
        .text
        .as_deref()
        .and_then(|text| text.strip_prefix("Material:").map(str::trim))
        .filter(|name| !name.is_empty())
    else {
        return Ok(());
    };
    let catalog = service.order_catalog().await?;
    let material = catalog
        .search_materials(material_name, 50)
        .await
        .map_err(TelegramError::OrderCatalog)?
        .into_iter()
        .find(|item| normalize_order_text(&item.name) == normalize_order_text(material_name));
    let Some(material) = material else {
        return Ok(());
    };
    let previous_prompt_message_id = draft.prompt_message_id.take();
    draft.pending_material_id = material.id;
    draft.pending_material_name = material.name;
    draft.step = TelegramOrderStep::Micron;
    draft.prompt_message_id = Some(message.message_id);
    draft.prompt_inline_message_id = None;
    service.save_order_draft(&telegram_user_id, draft).await?;
    if let Some(message_id) = previous_prompt_message_id
        && message_id != message.message_id
    {
        delete_message(
            service,
            token,
            &message.chat.id.to_string(),
            message_id,
        )
        .await
        .ok();
    }
    send_micron_step(service, token, &message.chat.id.to_string()).await
}

async fn show_customer_confirmation(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    customer_name: &str,
) -> Result<(), TelegramError> {
    let text = format!(
        "Mijoz: «{customer_name}»\n\n\
Tanlovni tasdiqlash uchun pastdagi «✅ Tanlash» tugmasini bosing.\n\
Boshqa mijoz tanlash uchun «👤 Mijoz» tugmasini yoki «❌ Bekor qilish» tugmasini bosing."
    );
    send_or_edit_order_prompt(
        service,
        token,
        chat_id,
        &text,
        Some(customer_confirmation_keyboard()),
    )
    .await
}

async fn show_product_confirmation(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    product_name: &str,
) -> Result<(), TelegramError> {
    let text = format!(
        "Mahsulot: «{product_name}»\n\n\
Tanlovni tasdiqlash uchun pastdagi «✅ Tanlash» tugmasini bosing.\n\
Boshqa mahsulot tanlash uchun «📦 Mahsulot» tugmasini yoki «❌ Bekor qilish» tugmasini bosing."
    );
    send_or_edit_order_prompt(
        service,
        token,
        chat_id,
        &text,
        Some(product_confirmation_keyboard()),
    )
    .await
}
