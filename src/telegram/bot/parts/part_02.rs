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
    let Some(draft) = service.order_draft(&telegram_user_id).await? else {
        return Ok(());
    };
    if draft.step != TelegramOrderStep::Attachment {
        send_order_text(
            service,
            token,
            &chat_id,
            "Rasm faqat tirajdan keyin yuboriladi. Order jarayonini davom ettiring.",
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
    let caption = order_caption(&draft.order_number, &draft, &account.display_name);
    let image = CalculateOrderImage {
        image_id: media.file_id,
        image_name: media.file_name,
        image_mime: media.mime_type,
        image_size_bytes: body.len() as u64,
        body,
    };
    match service
        .deliver_order(&telegram_user_id, &caption, Some(image))
        .await
    {
        Ok(0) => {
            send_order_text(
                service,
                token,
                &chat_id,
                "Order tayyor, lekin yuborish uchun guruh topilmadi. Guruhni ulang yoki tanlang.",
            )
            .await?;
        }
        Ok(count) => {
            service.clear_order_draft(&telegram_user_id).await?;
            send_order_text(
                service,
                token,
                &chat_id,
                &format!(
                    "✅ Order №T{} rasm bilan {} ta guruhga yuborildi.",
                    draft.order_number, count
                ),
            )
            .await?;
        }
        Err(error) => {
            send_order_text(
                service,
                token,
                &chat_id,
                &format!("Order yuborilmadi: {error}. Rasmni qayta yuborishingiz mumkin."),
            )
            .await?;
        }
    }
    Ok(())
}
