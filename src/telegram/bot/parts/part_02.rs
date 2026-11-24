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
    if let Some(search) = parse_alert_inline_query(&inline_query.query) {
        let (results, next_offset) = match alert_inline_results(
            service,
            &telegram_user_id,
            search,
            &inline_query.offset,
        )
        .await
        {
            Ok(page) => page,
            Err(error) => {
                tracing::warn!(?error, "telegram alert inline search failed");
                (Vec::new(), String::new())
            }
        };
        answer_inline_query_page(service, token, &inline_query.id, results, &next_offset).await?;
        return Ok(());
    }
    if account.user_profile_connected && parse_group_inline_query(&inline_query.query).is_some() {
        let results = match group_inline_results(service, &telegram_user_id, &inline_query.query).await {
            Ok(results) => results,
            Err(error) => {
                tracing::warn!(?error, "telegram group inline search failed");
                Vec::new()
            }
        };
        answer_inline_query(service, token, &inline_query.id, results).await?;
        return Ok(());
    }
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
            "Login jarayoni topilmadi. Avval /login orqali Telegram profilingizni ulashni boshlang.",
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
                    send_login_destination(service, token, &chat_id, &telegram_user_id).await?;
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
                    send_login_destination(service, token, &chat_id, &telegram_user_id).await?;
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
        Some("login") => {
            begin_bot_profile_login(service, token, &chat_id, &telegram_user_id).await?;
            return Ok(true);
        }
        Some("alerts") if account.role == TelegramAccountRole::AlertSender && !account.user_profile_connected => {
            begin_bot_profile_login(service, token, &chat_id, &telegram_user_id).await?;
            return Ok(true);
        }
        Some("alerts") => {
            show_alert_panel(service, token, &chat_id, &telegram_user_id, None).await?;
            return Ok(true);
        }
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
                send_user_group_picker(service, token, &chat_id).await?;
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
            send_user_group_picker(service, token, &chat_id).await?;
            return Ok(true);
        }
        Some("cancel") => {
            service.cancel_user_profile_login(&telegram_user_id).await;
            let had_order = service.order_draft(&telegram_user_id).await?.is_some();
            if had_order {
                clear_side_prompt(service, token, &chat_id).await?;
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
            send_user_group_picker(service, token, &chat_id).await?;
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
    let Some(mut draft) = service.order_draft(&telegram_user_id).await? else {
        return Ok(());
    };
    if draft.step == TelegramOrderStep::Side {
        send_side_step(service, token, &chat_id).await?;
        return Ok(());
    }
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
    draft.request_side();
    service
        .save_order_draft(&telegram_user_id, draft.clone())
        .await?;
    send_side_step(service, token, &chat_id).await?;
    Ok(())
}

include!("confirm_order.rs");
