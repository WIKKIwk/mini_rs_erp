async fn handle_order_text(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    telegram_user_id: &str,
    text: &str,
) -> Result<bool, TelegramError> {
    let Some(mut draft) = service.order_draft(telegram_user_id).await? else {
        return Ok(false);
    };
    let value = text.trim();
    let catalog = service.order_catalog().await?;
    match draft.step {
        TelegramOrderStep::CustomerName => {
            if value.is_empty() {
                send_order_text(service, token, chat_id, "Mijoz ismini kiriting.").await?;
                return Ok(true);
            }
            let (customer, created) = match catalog.find_customer_by_name(value).await {
                Ok(Some(customer)) => (customer, false),
                Ok(None) => (
                    catalog
                        .create_customer(value)
                        .await
                        .map_err(TelegramError::OrderCatalog)?,
                    true,
                ),
                Err(error) => return Err(TelegramError::OrderCatalog(error)),
            };
            draft.customer_ref = customer.ref_.clone();
            draft.customer_name = customer.name.clone();
            draft.step = TelegramOrderStep::Product;
            service.save_order_draft(telegram_user_id, draft).await?;
            let prefix = if created {
                format!("✅ Mijoz tizimga qo‘shildi: {}", customer.name)
            } else {
                format!(
                    "ℹ️ Bunday mijoz allaqachon bor: {}. Shu mijoz tanlandi.",
                    customer.name
                )
            };
            send_message_with_markup(
                service,
                token,
                chat_id,
                &format!("{prefix}\n\n📦 Mahsulot nomini tanlang:"),
                None,
                Some(product_step_keyboard()),
            )
            .await?;
        }
        TelegramOrderStep::ProductName => {
            if value.is_empty() {
                send_order_text(service, token, chat_id, "Mahsulot nomini kiriting.").await?;
                return Ok(true);
            }
            let item = match catalog
                .find_customer_item_by_name(&draft.customer_ref, value)
                .await
            {
                Ok(Some(item)) => (item, false),
                Ok(None) => (
                    catalog
                        .create_product(&draft.customer_ref, value)
                        .await
                        .map_err(TelegramError::OrderCatalog)?,
                    true,
                ),
                Err(error) => return Err(TelegramError::OrderCatalog(error)),
            };
            draft.product_code = item.0.code.clone();
            draft.product_name = item.0.name.clone();
            draft.step = TelegramOrderStep::Status;
            service.save_order_draft(telegram_user_id, draft).await?;
            let prefix = if item.1 {
                format!(
                    "✅ Mahsulot tayyor mahsulot kategoriyasiga qo‘shildi: {}",
                    item.0.name
                )
            } else {
                format!(
                    "ℹ️ Bu mahsulot allaqachon mavjud: {}. Shu mahsulot tanlandi.",
                    item.0.name
                )
            };
            send_message_with_markup(
                service,
                token,
                chat_id,
                &format!("{prefix}\n\nHolatni tanlang:"),
                None,
                Some(status_keyboard()),
            )
            .await?;
        }
        TelegramOrderStep::Tiraj => {
            let Some(tiraj) = parse_tiraj(value) else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Iltimos, tirajni raqamda yuboring (kg).",
                )
                .await?;
                return Ok(true);
            };
            draft.tiraj_kg = Some(tiraj);
            let order_number = if draft.order_number.trim().is_empty() {
                catalog
                    .next_order_number()
                    .await
                    .map_err(TelegramError::OrderCatalog)?
            } else {
                draft.order_number.clone()
            };
            draft.order_number = order_number.clone();
            draft.step = TelegramOrderStep::Attachment;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_order_text(
                service,
                token,
                chat_id,
                &format!(
                    "✅ Tiraj qabul qilindi: {tiraj} kg. Endi order rasmini photo yoki file ko‘rinishida yuboring. Rasm kelgach order guruhga yuboriladi."
                ),
            )
            .await?;
        }
        TelegramOrderStep::Attachment => {
            send_order_text(
                service,
                token,
                chat_id,
                "Orderni yuborish uchun rasm yoki rasm faylini yuboring.",
            )
            .await?;
        }
        _ => {
            send_order_text(
                service,
                token,
                chat_id,
                "Tanlovni pastdagi tugmalar orqali davom ettiring.",
            )
            .await?;
        }
    }
    Ok(true)
}

async fn handle_order_callback(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    telegram_user_id: &str,
    data: &str,
) -> Result<(), TelegramError> {
    let Some(mut draft) = service.order_draft(telegram_user_id).await? else {
        send_order_text(
            service,
            token,
            chat_id,
            "Joriy order jarayoni topilmadi. /new_order yuboring.",
        )
        .await?;
        return Ok(());
    };
    let payload = data.trim_start_matches("order:");
    let (action, value) = payload.split_once(':').unwrap_or((payload, ""));
    if action == "cancel" {
        service.clear_order_draft(telegram_user_id).await?;
        send_order_text(
            service,
            token,
            chat_id,
            "Order ochish jarayoni bekor qilindi.",
        )
        .await?;
        return Ok(());
    }
    let catalog = service.order_catalog().await?;
    match action {
        "add_customer" if draft.step == TelegramOrderStep::Customer => {
            draft.step = TelegramOrderStep::CustomerName;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_order_text(service, token, chat_id, "Mijoz ismini kiriting:").await?;
        }
        "customer" if draft.step == TelegramOrderStep::Customer => {
            let Some(customer_ref) = service.take_order_choice(telegram_user_id, value).await
            else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Mijoz tanlovi eskirgan. Qayta qidiring.",
                )
                .await?;
                return Ok(());
            };
            let customer = catalog
                .customer_by_ref(&customer_ref)
                .await
                .map_err(TelegramError::OrderCatalog)?;
            draft.customer_ref = customer.ref_.clone();
            draft.customer_name = customer.name.clone();
            draft.step = TelegramOrderStep::Product;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_product_step(service, token, chat_id, &customer.name).await?;
        }
        "add_product" if draft.step == TelegramOrderStep::Product => {
            draft.step = TelegramOrderStep::ProductName;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_order_text(service, token, chat_id, "Mahsulot nomini kiriting:").await?;
        }
        "product" if draft.step == TelegramOrderStep::Product => {
            let Some(item_code) = service.take_order_choice(telegram_user_id, value).await else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Mahsulot tanlovi eskirgan. Qayta qidiring.",
                )
                .await?;
                return Ok(());
            };
            let Some(item) = catalog
                .customer_item_by_code(&draft.customer_ref, &item_code)
                .await
                .map_err(TelegramError::OrderCatalog)?
            else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Bu mahsulot tanlangan mijozga ulanmagan.",
                )
                .await?;
                return Ok(());
            };
            draft.product_code = item.code;
            draft.product_name = item.name;
            draft.step = TelegramOrderStep::Status;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_status_step(service, token, chat_id).await?;
        }
        "status" if draft.step == TelegramOrderStep::Status => {
            draft.status = match value {
                "roll" => "rulon".to_string(),
                "package" => "paket".to_string(),
                _ => return Ok(()),
            };
            draft.step = TelegramOrderStep::Material;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_material_step(service, token, chat_id, 1).await?;
        }
        "material" if draft.step == TelegramOrderStep::Material => {
            let Some(material_id) = service.take_order_choice(telegram_user_id, value).await else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Material tanlovi eskirgan. Qayta qidiring.",
                )
                .await?;
                return Ok(());
            };
            let Some(material) = catalog
                .material_by_id(&material_id)
                .await
                .map_err(TelegramError::OrderCatalog)?
            else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Material topilmadi. Qayta tanlang.",
                )
                .await?;
                return Ok(());
            };
            draft.pending_material_id = material.id;
            draft.pending_material_name = material.name;
            draft.step = TelegramOrderStep::Micron;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_micron_step(service, token, chat_id).await?;
        }
        "micron" if draft.step == TelegramOrderStep::Micron => {
            let Some(micron) = service.take_order_choice(telegram_user_id, value).await else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Mikron tanlovi eskirgan. Qayta qidiring.",
                )
                .await?;
                return Ok(());
            };
            let Some(material) = catalog
                .material_by_id(&draft.pending_material_id)
                .await
                .map_err(TelegramError::OrderCatalog)?
            else {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Material topilmadi. Qavatni qayta tanlang.",
                )
                .await?;
                return Ok(());
            };
            if !material
                .variants
                .iter()
                .any(|variant| variant.micron.to_string() == micron)
            {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Bu mikron tanlangan materialda mavjud emas.",
                )
                .await?;
                return Ok(());
            }
            draft.layers.push(TelegramOrderLayer {
                material_id: material.id,
                material: draft.pending_material_name.clone(),
                micron,
            });
            draft.pending_material_id.clear();
            draft.pending_material_name.clear();
            draft.step = TelegramOrderStep::LayerOptions;
            service
                .save_order_draft(telegram_user_id, draft.clone())
                .await?;
            send_layer_options(service, token, chat_id, &draft).await?;
        }
        "add_layer" if draft.step == TelegramOrderStep::LayerOptions => {
            draft.step = TelegramOrderStep::Material;
            let layer_number = draft.layers.len() + 1;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_material_step(service, token, chat_id, layer_number).await?;
        }
        "next_layers" if draft.step == TelegramOrderStep::LayerOptions => {
            draft.step = TelegramOrderStep::Tiraj;
            service.save_order_draft(telegram_user_id, draft).await?;
            send_order_text(
                service,
                token,
                chat_id,
                "Tirajni kg da raqam bilan yuboring:",
            )
            .await?;
        }
        _ => {}
    }
    Ok(())
}
