fn order_media_from_message(message: &TelegramMessage) -> Option<TelegramOrderMedia> {
    if let Some(photo) = message.photo.as_ref().and_then(|photos| {
        photos.iter().max_by_key(|photo| {
            (
                photo.width.saturating_mul(photo.height),
                photo.file_size.unwrap_or_default(),
            )
        })
    }) {
        return Some(TelegramOrderMedia {
            file_id: photo.file_id.clone(),
            file_name: "order-image.jpg".to_string(),
            mime_type: "image/jpeg".to_string(),
            file_size: photo.file_size,
        });
    }
    let document = message.document.as_ref()?;
    let file_name = document
        .file_name
        .clone()
        .unwrap_or_else(|| "order-image".to_string());
    let mime_type = document.mime_type.clone().unwrap_or_default();
    if !is_image_file(&mime_type, &file_name) {
        return None;
    }
    Some(TelegramOrderMedia {
        file_id: document.file_id.clone(),
        file_name: file_name.clone(),
        mime_type: if mime_type.trim().is_empty() {
            image_mime_from_name(&file_name)
        } else {
            mime_type
        },
        file_size: document.file_size,
    })
}

fn is_image_file(mime_type: &str, file_name: &str) -> bool {
    if mime_type.trim().to_ascii_lowercase().starts_with("image/") {
        return true;
    }
    matches!(
        file_name
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp"
    )
}

fn image_mime_from_name(file_name: &str) -> String {
    match file_name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => "image/jpeg",
    }
    .to_string()
}

async fn handle_contact(
    service: &TelegramService,
    token: &str,
    message: &TelegramMessage,
    contact: &TelegramContact,
) -> Result<(), TelegramError> {
    let Some(user) = message.from.as_ref() else {
        return Ok(());
    };
    let chat_id = message.chat.id.to_string();
    if contact.user_id != Some(user.id) {
        send_message(
            service,
            token,
            &chat_id,
            "Xavfsizlik uchun faqat o‘zingizning Telegram contact’ingizni yuboring.",
            None,
        )
        .await?;
        return Ok(());
    }
    match service
        .begin_user_profile_login(&user.id.to_string(), &contact.phone_number)
        .await
    {
        Ok(LoginOutcome::CodeSent) => {
            send_inline_login_prompt(
                service,
                token,
                &chat_id,
                "📩 Telegram login kodi yuborildi. Kodni chatga oddiy xabar qilib yubormang. Pastdagi tugmani bosing va kodni inline maydoniga joylab yuboring.",
                INLINE_CODE_PREFIX,
                "🔐 Kodni inline yuborish",
            )
            .await?;
        }
        Ok(LoginOutcome::Authorized) => {
            send_user_group_picker(service, token, &chat_id, &user.id.to_string()).await?;
        }
        Err(error) => {
            send_message(
                service,
                token,
                &chat_id,
                &user_account_error_message(&error),
                None,
            )
            .await?;
        }
    }
    Ok(())
}

async fn handle_callback_query(
    service: &TelegramService,
    token: &str,
    callback: TelegramCallbackQuery,
) -> Result<(), TelegramError> {
    answer_callback_query(service, token, &callback.id, None, false).await?;
    let Some(data) = callback.data.as_deref() else {
        return Ok(());
    };
    let telegram_user_id = callback.from.id.to_string();
    let chat_id = callback
        .message
        .as_ref()
        .map(|message| message.chat.id.to_string())
        .unwrap_or_else(|| telegram_user_id.clone());
    if data.starts_with("order:") {
        handle_order_callback(service, token, &chat_id, &telegram_user_id, data).await?;
        return Ok(());
    }
    match data {
        "delivery:bot" => {
            service
                .set_delivery_mode(&telegram_user_id, TelegramDeliveryMode::Bot)
                .await?;
            send_message_with_markup(
                service,
                token,
                &chat_id,
                "✅ Delivery mode: Bot orqali tanlandi. Orderlar bot ulangan guruhlarga yuboriladi.",
                None,
                Some(remove_keyboard_markup()),
            )
            .await?;
        }
        "delivery:user" => {
            let account = service
                .set_delivery_mode(&telegram_user_id, TelegramDeliveryMode::UserProfile)
                .await?;
            if account.user_profile_connected {
                send_user_group_picker(service, token, &chat_id, &telegram_user_id).await?;
            } else {
                send_contact_request(service, token, &chat_id).await?;
            }
        }
        value if value.starts_with("user_group:") => {
            let Some((chat_type, chat_id_value)) =
                value.trim_start_matches("user_group:").split_once(':')
            else {
                return Ok(());
            };
            match service
                .select_user_group(&telegram_user_id, chat_id_value, chat_type)
                .await
            {
                Ok(account) => {
                    send_message_with_markup(
                        service,
                        token,
                        &chat_id,
                        &format!(
                            "✅ User profile ulandi. Orderlar faqat «{}» guruhiga yuboriladi.",
                            account
                                .selected_chat_title
                                .as_deref()
                                .unwrap_or("tanlangan guruh")
                        ),
                        None,
                        Some(remove_keyboard_markup()),
                    )
                    .await?;
                }
                Err(error) => {
                    send_message(
                        service,
                        token,
                        &chat_id,
                        &user_account_error_message(&error),
                        None,
                    )
                    .await?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

async fn send_contact_request(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
) -> Result<(), TelegramError> {
    send_message_with_markup(
        service,
        token,
        chat_id,
        "👤 User profile orqali yuborish uchun Telegram profilingizni ulaymiz. Pastdagi tugmani bosib, o‘zingizning telefon raqamingizni yuboring.",
        None,
        Some(contact_request_markup()),
    )
    .await
}
async fn send_user_group_picker(
    service: &TelegramService,
    token: &str,
    chat_id: &str,
    telegram_user_id: &str,
) -> Result<(), TelegramError> {
    let groups = match service.writable_user_groups(telegram_user_id).await {
        Ok(groups) => groups,
        Err(error) => {
            send_message(
                service,
                token,
                chat_id,
                &user_account_error_message(&error),
                None,
            )
            .await?;
            return Ok(());
        }
    };
    if groups.is_empty() {
        send_message_with_markup(
            service,
            token,
            chat_id,
            "Siz yozish huquqiga ega bo‘lgan guruh topilmadi. Telegram profilingizni guruhga qo‘shing yoki admin huquqini tekshiring.",
            None,
            Some(remove_keyboard_markup()),
        )
        .await?;
        return Ok(());
    }
    send_message_with_markup(
        service,
        token,
        chat_id,
        "Guruh tanlang. User profile orderlarni boshqa chatlarga yubormaydi:",
        None,
        Some(user_group_keyboard(&groups)),
    )
    .await
}

async fn connect_group(
    service: &TelegramService,
    token: &str,
    message: &TelegramMessage,
) -> Result<(), TelegramError> {
    if !matches!(
        message.chat.chat_type.as_str(),
        "group" | "supergroup" | "channel"
    ) {
        return Ok(());
    }
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    service
        .connect_chat(TelegramChat {
            chat_id: message.chat.id.to_string(),
            title: message
                .chat
                .title
                .clone()
                .or_else(|| message.chat.username.clone())
                .unwrap_or_else(|| "Telegram group".to_string()),
            username: message
                .chat
                .username
                .clone()
                .unwrap_or_default()
                .trim_start_matches('@')
                .to_string(),
            chat_type: message.chat.chat_type.clone(),
            thread_id: message.message_thread_id,
            connected_at_unix: now,
            last_seen_at_unix: now,
        })
        .await?;
    send_message(
        service,
        token,
        &message.chat.id.to_string(),
        "Guruh ulandi. Yangi orderlar shu yerga bot orqali yuboriladi.",
        message.message_thread_id,
    )
    .await
}
