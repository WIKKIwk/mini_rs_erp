async fn confirm_order(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    telegram_user_id: &str,
) -> Result<(), TelegramError> {
    let Some(account) = service.user_by_telegram_id(telegram_user_id).await? else {
        return Ok(());
    };
    if account.role != TelegramAccountRole::SalesManager {
        return Ok(());
    }
    let Some(mut draft) = service.order_draft(telegram_user_id).await? else {
        send_order_text(service, token, chat_id, "Joriy order jarayoni topilmadi.").await?;
        return Ok(());
    };
    if draft.step != TelegramOrderStep::Review {
        send_order_text(
            service,
            token,
            chat_id,
            "Avval order ma’lumotlarini to‘liq kiriting va rasm yuboring.",
        )
        .await?;
        return Ok(());
    }
    if !draft.pending_order_saved && !draft.side.is_some_and(|side| (1..=4).contains(&side)) {
        draft.request_side();
        service.save_order_draft(telegram_user_id, draft).await?;
        send_side_step(service, token, chat_id).await?;
        return Ok(());
    }

    let (delivery_image, saved_message) = if draft.pending_order_saved {
        let image = if let Some(attachment) = service.order_attachment(telegram_user_id).await {
            CalculateOrderImage {
                image_id: format!(
                    "telegram-order-{}-{:032x}",
                    draft.order_number,
                    rand::random::<u128>()
                ),
                image_name: attachment.file_name,
                image_mime: attachment.mime_type,
                image_size_bytes: attachment.body.len() as u64,
                body: attachment.body,
            }
        } else {
            match service
                .pending_order_image(&format!("zakaz-{}", draft.order_number))
                .await
            {
                Ok(Some(image)) => image,
                Ok(None) => {
                    draft.pending_order_saved = false;
                    draft.step = TelegramOrderStep::Attachment;
                    service
                        .save_order_draft(telegram_user_id, draft)
                        .await?;
                    send_order_text(
                        service,
                        token,
                        chat_id,
                        "Order rasmi topilmadi. Rasmni qayta yuboring.",
                    )
                    .await?;
                    return Ok(());
                }
                Err(error) => {
                    send_order_text(
                        service,
                        token,
                        chat_id,
                        &format!("Order rasmi olinmadi: {error}. Rasmni qayta yuboring."),
                    )
                    .await?;
                    return Ok(());
                }
            }
        };
        (
            image,
            format!("✅ Order №T{} avval saqlangan.", draft.order_number),
        )
    } else {
        let Some(attachment) = service.order_attachment(telegram_user_id).await else {
            draft.step = TelegramOrderStep::Attachment;
            service
                .save_order_draft(telegram_user_id, draft)
                .await?;
            send_order_text(
                service,
                token,
                chat_id,
                "Order rasmi topilmadi. Iltimos, rasmni qayta yuboring.",
            )
            .await?;
            return Ok(());
        };
        let catalog = service.order_catalog().await?;
        if draft.customer_ref.trim().is_empty() {
            if draft.customer_name.trim().is_empty() {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Mijoz nomi topilmadi. Tahrirlash orqali mijozni kiriting.",
                )
                .await?;
                return Ok(());
            }
            match catalog.find_customer_by_name(&draft.customer_name).await {
                Ok(Some(customer)) => {
                    draft.customer_ref = customer.ref_.clone();
                    draft.customer_name = customer.name.clone();
                }
                Ok(None) => match catalog.create_customer(&draft.customer_name).await {
                    Ok(customer) => {
                        draft.customer_ref = customer.ref_.clone();
                        draft.customer_name = customer.name.clone();
                    }
                    Err(error) => {
                        send_order_text(
                            service,
                            token,
                            chat_id,
                            &format!("Mijoz yaratilmadi: {error}. Qayta urinib ko‘ring."),
                        )
                        .await?;
                        return Ok(());
                    }
                },
                Err(error) => return Err(TelegramError::OrderCatalog(error)),
            }
        }
        if draft.product_code.trim().is_empty() {
            if draft.product_name.trim().is_empty() {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Mahsulot nomi topilmadi. Tahrirlash orqali mahsulotni kiriting.",
                )
                .await?;
                return Ok(());
            }
            match catalog
                .find_customer_item_by_name(&draft.customer_ref, &draft.product_name)
                .await
            {
                Ok(Some(item)) => {
                    draft.product_code = item.code;
                    draft.product_name = item.name;
                }
                Ok(None) => match catalog
                    .create_product(&draft.customer_ref, &draft.product_name)
                    .await
                {
                    Ok(item) => {
                        draft.product_code = item.code;
                        draft.product_name = item.name;
                    }
                    Err(error) => {
                        send_order_text(
                            service,
                            token,
                            chat_id,
                            &format!("Mahsulot yaratilmadi: {error}. Qayta urinib ko‘ring."),
                        )
                        .await?;
                        return Ok(());
                    }
                },
                Err(error) => return Err(TelegramError::OrderCatalog(error)),
            }
        }
        if draft.customer_name.trim().is_empty()
            || draft.product_name.trim().is_empty()
            || draft.status != "rulon" && draft.status != "paket"
            || draft.print_method.is_none()
            || draft.cold_glue.is_none()
            || draft.layers.is_empty()
            || draft
                .layers
                .iter()
                .any(|layer| layer.material_id.trim().is_empty() || layer.micron.trim().is_empty())
            || draft.tiraj_kg.is_none_or(|value| !value.is_finite() || value <= 0.0)
            || draft
                .frame_product_size_mm
                .is_none_or(|value| !value.is_finite() || value <= 0.0)
            || draft
                .frame_count
                .is_none_or(|value| !value.is_finite() || value <= 0.0 || value.fract() != 0.0)
            || draft.diameter_mm.is_none_or(|value| !value.is_finite() || value <= 0.0)
            || draft.roll_count.is_none_or(|value| value <= 0)
            || (matches!(
                draft.print_method,
                Some(crate::core::production_map::automatic::PrintMethod::Flexo)
            ) && draft.edge_allowance_mm.is_none_or(|value| {
                !value.is_finite() || value < 0.0
            }))
        {
            send_order_text(
                service,
                token,
                chat_id,
                "Order ma’lumotlari to‘liq emas. Tahrirlash orqali yetishmayotgan bo‘limni to‘ldiring.",
            )
            .await?;
            return Ok(());
        }
        service
            .save_order_draft(telegram_user_id, draft.clone())
            .await?;

        let original_body = attachment.body.clone();
        let original_file_name = attachment.file_name.clone();
        let original_mime_type = attachment.mime_type.clone();
        tracing::debug!(mime = %original_mime_type, "optimizing Telegram order image for ERP storage");
        let optimized = match tokio::task::spawn_blocking(move || {
            crate::http::handlers::calculate_image::optimize_order_image_for_store(
                &original_body,
                &original_file_name,
            )
        })
        .await
        {
            Ok(Ok(image)) => image,
            _ => {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    "Rasmni ochib bo‘lmadi. Boshqa JPG yoki PNG rasm yuboring.",
                )
                .await?;
                return Ok(());
            }
        };
        let image_id = format!(
            "telegram-order-{}-{:032x}",
            draft.order_number,
            rand::random::<u128>()
        );
        let delivery_image = CalculateOrderImage {
            image_id: image_id.clone(),
            image_name: attachment.file_name,
            image_mime: attachment.mime_type,
            image_size_bytes: attachment.body.len() as u64,
            body: attachment.body,
        };
        let storage_image = CalculateOrderImage {
            image_id,
            image_name: optimized.file_name,
            image_mime: "image/webp".into(),
            image_size_bytes: optimized.body.len() as u64,
            body: optimized.body,
        };
        let intake = match service
            .persist_pending_order(&account, &draft, storage_image)
            .await
        {
            Ok(intake) => intake,
            Err(error) => {
                send_order_text(
                    service,
                    token,
                    chat_id,
                    &format!("Order saqlanmadi: {error}. Qayta urinib ko‘ring."),
                )
                .await?;
                return Ok(());
            }
        };
        draft.pending_order_saved = true;
        service
            .save_order_draft(telegram_user_id, draft.clone())
            .await?;
        (
            delivery_image,
            intake.message(&draft.order_number),
        )
    };

    let caption = order_caption(&draft.order_number, &draft, &account.display_name);
    match service
        .deliver_order(telegram_user_id, &caption, Some(delivery_image))
        .await
    {
        Ok(0) => {
            send_order_text(
                service,
                token,
                chat_id,
                &format!(
                    "{saved_message}\nGuruh tanlanmagan. /groups orqali guruhni tanlang va qayta tasdiqlang."
                ),
            )
            .await?;
        }
        Ok(count) => {
            send_order_text(
                service,
                token,
                chat_id,
                &format!("{saved_message}\nRasm bilan {count} ta guruhga yuborildi."),
            )
            .await?;
            service.clear_order_draft(telegram_user_id).await?;
        }
        Err(error) => {
            send_order_text(
                service,
                token,
                chat_id,
                &format!(
                    "{saved_message}\nGuruhga yuborilmadi: {error}. Guruhni tekshirib, qayta tasdiqlang."
                ),
            )
            .await?;
        }
    }
    Ok(())
}
