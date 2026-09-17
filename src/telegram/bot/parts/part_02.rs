async fn handle_inline_query(
    service: &TelegramService,
    token: &str,
    inline_query: TelegramInlineQuery,
) -> Result<(), TelegramError> {
    let telegram_user_id = inline_query.from.id.to_string();
    let Some(account) = service.user_by_telegram_id(&telegram_user_id).await? else {
        answer_inline_query(service, token, &inline_query.id, Vec::new()).await?;
        return Ok(());
    };
    if account.role == TelegramAccountRole::SalesManager
        && parse_order_inline_query(&inline_query.query).is_some()
        && service.order_draft(&telegram_user_id).await?.is_some()
    {
        let results =
            match order_inline_results(service, &telegram_user_id, &inline_query.query).await {
                Ok(results) => results,
                Err(error) => {
                    tracing::warn!(?error, "telegram order inline search failed");
                    Vec::new()
                }
            };
        answer_inline_query(service, token, &inline_query.id, results).await?;
        return Ok(());
    }
    answer_inline_query(service, token, &inline_query.id, Vec::new()).await?;

    let Some(input) = parse_inline_login_input(&inline_query.query) else {
        return Ok(());
    };
    let chat_id = if account.telegram_chat_id.trim().is_empty() {
        telegram_user_id.clone()
    } else {
        account.telegram_chat_id.clone()
    };
    if !service
        .has_pending_user_profile_login(&telegram_user_id)
        .await
    {
        send_message(
            service,
            token,
            &chat_id,
            "Login jarayoni topilmadi. Avval /user_mode orqali user profile ulashni boshlang.",
            None,
        )
        .await?;
        return Ok(());
    }

    match input {
        InlineLoginInput::Code(code) => {
            match service
                .complete_user_profile_code(&telegram_user_id, &code)
                .await
            {
                Ok(CodeOutcome::PasswordRequired { hint }) => {
                    let hint = hint
                        .map(|hint| format!(" (hint: {hint})"))
                        .unwrap_or_default();
                    send_inline_login_prompt(
                        service,
                        token,
                        &chat_id,
                        &format!(
                            "🔐 2FA parol kerak{hint}. Parolni oddiy chat xabari qilib yubormang."
                        ),
                        INLINE_PASSWORD_PREFIX,
                        "🔐 2FA parolni inline yuborish",
                    )
                    .await?;
                }
                Ok(CodeOutcome::Authorized) => {
                    send_user_group_picker(service, token, &chat_id, &telegram_user_id).await?;
                }
                Err(error) => {
                    send_inline_login_prompt(
                        service,
                        token,
                        &chat_id,
                        &user_account_error_message(&error),
                        INLINE_CODE_PREFIX,
                        "🔐 Kodni inline yuborish",
                    )
                    .await?;
                }
            }
        }
        InlineLoginInput::Password(password) => {
            match service
                .complete_user_profile_password(&telegram_user_id, &password)
                .await
            {
                Ok(_) => {
                    send_user_group_picker(service, token, &chat_id, &telegram_user_id).await?;
                }
                Err(error) => {
                    send_inline_login_prompt(
                        service,
                        token,
                        &chat_id,
                        &user_account_error_message(&error),
                        INLINE_PASSWORD_PREFIX,
                        "🔐 2FA parolni inline yuborish",
                    )
                    .await?;
                }
            }
        }
    }
    Ok(())
}
async fn handle_private_text(
    service: &TelegramService,
    token: &str,
    message: &TelegramMessage,
    text: &str,
) -> Result<bool, TelegramError> {
    let Some(user) = message.from.as_ref() else {
        return Ok(false);
    };
    let telegram_user_id = user.id.to_string();
    let Some(account) = service.user_by_telegram_id(&telegram_user_id).await? else {
        return Ok(false);
    };
    let chat_id = message.chat.id.to_string();
    let parsed_command = parse_command(text);
    let command = parsed_command.as_ref().map(|(command, _)| command.as_str());

    match command {
        // /start must reach account recognition even while an order draft exists.
        Some("start") => return Ok(false),
        Some("new_order") if account.role == TelegramAccountRole::SalesManager => {
            start_new_order(service, token, &telegram_user_id, &chat_id).await?;
            return Ok(true);
        }
        Some("bot_mode") | Some("bot") if account.role == TelegramAccountRole::SalesManager => {
            service
                .set_delivery_mode(&telegram_user_id, TelegramDeliveryMode::Bot)
                .await?;
            send_message_with_markup(
                service,
                token,
                &chat_id,
                "✅ Delivery mode: Bot orqali. Orderlar bot ulangan guruhlarga yuboriladi.",
                None,
                Some(remove_keyboard_markup()),
            )
            .await?;
            return Ok(true);
        }
        Some("user_mode") | Some("userbot")
            if account.role == TelegramAccountRole::SalesManager =>
        {
            service
                .set_delivery_mode(&telegram_user_id, TelegramDeliveryMode::UserProfile)
                .await?;
            if account.user_profile_connected {
                send_user_group_picker(service, token, &chat_id, &telegram_user_id).await?;
            } else {
                send_contact_request(service, token, &chat_id).await?;
            }
            return Ok(true);
        }
        Some("code") => {
            send_inline_login_prompt(
                service,
                token,
                &chat_id,
                "Login kodini chatga oddiy xabar qilib yubormang. Pastdagi tugmani bosing va kodni inline maydoniga joylab yuboring.",
                INLINE_CODE_PREFIX,
                "🔐 Kodni inline yuborish",
            )
            .await?;
            return Ok(true);
        }
        Some("password") => {
            send_inline_login_prompt(
                service,
                token,
                &chat_id,
                "2FA parolini chatga oddiy xabar qilib yubormang. Pastdagi tugmani bosing va parolni inline maydoniga joylab yuboring.",
                INLINE_PASSWORD_PREFIX,
                "🔐 2FA parolni inline yuborish",
            )
            .await?;
            return Ok(true);
        }
        Some("groups") if account.user_profile_connected => {
            send_user_group_picker(service, token, &chat_id, &telegram_user_id).await?;
            return Ok(true);
        }
        Some("cancel") => {
            service.cancel_user_profile_login(&telegram_user_id).await;
            let had_order = service.order_draft(&telegram_user_id).await?.is_some();
            if had_order {
                service.clear_order_draft(&telegram_user_id).await?;
            }
            send_message_with_markup(
                service,
                token,
                &chat_id,
                if had_order {
                    "Order ochish jarayoni bekor qilindi."
                } else {
                    "Login jarayoni bekor qilindi."
                },
                None,
                Some(remove_keyboard_markup()),
            )
            .await?;
            return Ok(true);
        }
        _ => {}
    }

    if text == "🤖 Bot orqali" && account.role == TelegramAccountRole::SalesManager {
        service
            .set_delivery_mode(&telegram_user_id, TelegramDeliveryMode::Bot)
            .await?;
        send_message_with_markup(
            service,
            token,
            &chat_id,
            "✅ Delivery mode: Bot orqali tanlandi.",
            None,
            Some(remove_keyboard_markup()),
        )
        .await?;
        return Ok(true);
    }
    if text == "👤 User profile orqali" && account.role == TelegramAccountRole::SalesManager {
        service
            .set_delivery_mode(&telegram_user_id, TelegramDeliveryMode::UserProfile)
            .await?;
        if account.user_profile_connected {
            send_user_group_picker(service, token, &chat_id, &telegram_user_id).await?;
        } else {
            send_contact_request(service, token, &chat_id).await?;
        }
        return Ok(true);
    }
    if text == "📱 Telefon raqamini yuborish" {
        send_contact_request(service, token, &chat_id).await?;
        return Ok(true);
    }
    if service.order_draft(&telegram_user_id).await?.is_some() {
        return handle_order_text(service, token, &chat_id, &telegram_user_id, text).await;
    }
    if service
        .has_pending_user_profile_login(&telegram_user_id)
        .await
        && is_login_code(text)
    {
        // Kod oddiy xabar bo'lib tarixda qolmasligi uchun avval o'chiramiz
        // (huquq bo'lmasa e'tiborsiz), keyin inline yo'lni ko'rsatamiz.
        delete_message(service, token, &chat_id, message.message_id)
            .await
            .ok();
        send_inline_login_prompt(
            service,
            token,
            &chat_id,
            "Login kodini chatga oddiy xabar qilib yubormang. Pastdagi tugmani bosing va kodni inline maydoniga joylab yuboring.",
            INLINE_CODE_PREFIX,
            "🔐 Kodni inline yuborish",
        )
        .await?;
        return Ok(true);
    }
    Ok(false)
}

async fn handle_private_media(
    service: &TelegramService,
    token: &str,
    message: &TelegramMessage,
) -> Result<(), TelegramError> {
    let Some(user) = message.from.as_ref() else {
        return Ok(());
    };
    let telegram_user_id = user.id.to_string();
    let Some(account) = service.user_by_telegram_id(&telegram_user_id).await? else {
        return Ok(());
    };
    if account.role != TelegramAccountRole::SalesManager {
        return Ok(());
    }
    let chat_id = message.chat.id.to_string();
    let Some(mut draft) = service.order_draft(&telegram_user_id).await? else {
        return Ok(());
    };
    if !matches!(draft.step, TelegramOrderStep::Attachment | TelegramOrderStep::Review) {
        send_order_text(
            service,
            token,
            &chat_id,
            "Rasm faqat orderni yakunlash bosqichida yuboriladi. Order jarayonini davom ettiring.",
        )
        .await?;
        return Ok(());
    }
    if draft.pending_order_saved {
        send_order_text(
            service,
            token,
            &chat_id,
            "Order allaqachon saqlangan. Uni yuborish uchun quyidagi tasdiqlash tugmasini bosing.",
        )
        .await?;
        return Ok(());
    }
    let Some(media) = order_media_from_message(message) else {
        send_order_text(
            service,
            token,
            &chat_id,
            "Iltimos, order uchun rasm yoki rasm faylini yuboring.",
        )
        .await?;
        return Ok(());
    };
    if media
        .file_size
        .is_some_and(|size| size > MAX_ORDER_IMAGE_BYTES)
    {
        send_order_text(
            service,
            token,
            &chat_id,
            "Rasm hajmi 20 MB dan oshmasin. Boshqa rasm yuboring.",
        )
        .await?;
        return Ok(());
    }
    let telegram_file = get_telegram_file(service, token, &media.file_id).await?;
    let Some(file_path) = telegram_file.file_path else {
        send_order_text(
            service,
            token,
            &chat_id,
            "Rasmni Telegram serveridan olishning iloji bo‘lmadi. Qayta yuboring.",
        )
        .await?;
        return Ok(());
    };
    let body = download_telegram_file(service, token, &file_path).await?;
    if body.is_empty() || body.len() as u64 > MAX_ORDER_IMAGE_BYTES {
        send_order_text(
            service,
            token,
            &chat_id,
            "Rasm hajmi 20 MB dan oshmasin. Boshqa rasm yuboring.",
        )
        .await?;
        return Ok(());
    }
    if draft.order_number.trim().is_empty() {
        draft.order_number = service
            .order_catalog()
            .await?
            .next_order_number()
            .await
            .map_err(TelegramError::OrderCatalog)?;
    }
    service
        .save_order_attachment(
            &telegram_user_id,
            TelegramOrderAttachment {
                file_name: media.file_name,
                mime_type: media.mime_type,
                body,
            },
        )
        .await;
    draft.edit_section = None;
    draft.step = TelegramOrderStep::Review;
    service
        .save_order_draft(&telegram_user_id, draft.clone())
        .await?;
    send_order_review(service, token, &chat_id, &telegram_user_id, &draft).await?;
    Ok(())
}

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
            service.clear_order_draft(telegram_user_id).await?;
            send_order_text(
                service,
                token,
                chat_id,
                &format!("{saved_message}\nRasm bilan {count} ta guruhga yuborildi."),
            )
            .await?;
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
