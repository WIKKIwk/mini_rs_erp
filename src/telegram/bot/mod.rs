//! Telegram Bot API transport.
//!
//! This module owns the bot-facing transport only. User-account sessions live
//! in `telegram::useraccount` and are intentionally not mixed with bot tokens.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::calculate_orders::{CalculateOrderImage, CalculateOrderTemplate};
use crate::core::production_map::ProductionMapDefinition;

use super::models::{
    TelegramAccountRole, TelegramChat, TelegramDeliveryMode, TelegramStartRequest,
    TelegramUserGroup,
};
use super::service::{TelegramError, TelegramService};
use super::useraccount::{CodeOutcome, LoginOutcome};

const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";
const POLL_TIMEOUT_SECONDS: u64 = 25;
const INLINE_CODE_PREFIX: &str = "q7 ";
const INLINE_PASSWORD_PREFIX: &str = "p4 ";

#[derive(Debug, Clone)]
pub(crate) struct TelegramOrderNotification {
    pub(crate) caption: String,
    pub(crate) image: Option<CalculateOrderImage>,
}

impl TelegramOrderNotification {
    pub(crate) fn from_order(
        map: ProductionMapDefinition,
        template: CalculateOrderTemplate,
        image: Option<CalculateOrderImage>,
        sender_display_name: String,
    ) -> Self {
        let order_number = first_non_empty(&[
            template.order_number.as_str(),
            map.order_number.as_str(),
            map.code.as_str(),
            map.id.as_str(),
        ]);
        let customer = first_non_empty(&[template.customer.as_str(), map.customer_name.as_str()]);
        let product = first_non_empty(&[template.product.as_str(), map.title.as_str()]);
        let status = if template.status.trim().is_empty() {
            "Yangi"
        } else {
            template.status.trim()
        };
        let material = if template.material_display.trim().is_empty() {
            let layers = template
                .effective_layers()
                .into_iter()
                .filter_map(|layer| {
                    let material = layer.material.trim();
                    let micron = layer.micron.trim();
                    if material.is_empty() && micron.is_empty() {
                        None
                    } else if micron.is_empty() {
                        Some(material.to_string())
                    } else {
                        Some(format!("{material} {micron}"))
                    }
                })
                .collect::<Vec<_>>();
            if layers.is_empty() {
                "—".to_string()
            } else {
                layers.join(" + ")
            }
        } else {
            template.material_display.trim().to_string()
        };
        let width = if template.width_mm > 0.0 {
            template.width_mm
        } else {
            map.width_mm.unwrap_or_default()
        };
        let note = if template.note.trim().is_empty() {
            "—"
        } else {
            template.note.trim()
        };
        let sender = if sender_display_name.trim().is_empty() {
            "Mini RS ERP"
        } else {
            sender_display_name.trim()
        };
        let caption = format!(
            "📦 Buyurtma raqami: №{order_number}\n\
Mijoz: {customer}\n\
Mahsulot: {product}\n\
Holat: {status}\n\n\
1. Material: {material}\n\
2. Rang: {}\n\
3. Tiraj: {} kg\n\
4. Menejer: {sender}\n\
5. O‘lcham: {} mm × {} mm\n\
6. Kadr soni: {} ta\n\n\
Izoh: {note}",
            empty_dash(&template.color),
            number_or_dash(template.kg),
            number_or_dash(template.frame_product_size_mm),
            number_or_dash(width),
            number_or_dash(template.frame_count),
        );
        Self { caption, image }
    }
}

#[derive(Debug, Deserialize)]
struct TelegramApiResponse<T> {
    ok: bool,
    #[serde(default)]
    result: Option<T>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    #[serde(default)]
    message: Option<TelegramMessage>,
    #[serde(default)]
    callback_query: Option<TelegramCallbackQuery>,
    #[serde(default)]
    inline_query: Option<TelegramInlineQuery>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    chat: TelegramMessageChat,
    #[serde(default)]
    from: Option<TelegramUser>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    contact: Option<TelegramContact>,
    #[serde(default)]
    message_thread_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TelegramContact {
    phone_number: String,
    #[serde(default)]
    user_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessageChat {
    id: i64,
    #[serde(rename = "type")]
    chat_type: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    id: i64,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    first_name: String,
    #[serde(default)]
    last_name: String,
}

#[derive(Debug, Deserialize)]
struct TelegramCallbackQuery {
    id: String,
    from: TelegramUser,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramInlineQuery {
    id: String,
    from: TelegramUser,
    #[serde(default)]
    query: String,
}

#[derive(Debug, Serialize)]
struct GetUpdatesRequest {
    offset: i64,
    timeout: u64,
    allowed_updates: [&'static str; 3],
}

pub(crate) async fn run_polling(service: TelegramService) {
    let mut offset = service.update_offset().await.unwrap_or_default();
    loop {
        let (_, token) = match service.bot_credentials().await {
            Ok(credentials) => credentials,
            Err(error) => {
                tracing::warn!(?error, "telegram bot settings unavailable");
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
        };
        if token.trim().is_empty() {
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        }
        match get_updates(&service, &token, offset).await {
            Ok(updates) => {
                for update in updates {
                    offset = offset.max(update.update_id.saturating_add(1));
                    if let Err(error) = handle_update(&service, &token, update).await {
                        tracing::warn!(?error, "telegram update handling failed");
                    }
                    if let Err(error) = service.set_update_offset(offset).await {
                        tracing::warn!(?error, offset, "telegram update offset persist failed");
                    }
                }
            }
            Err(error) => {
                tracing::warn!(?error, "telegram polling failed");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn get_updates(
    service: &TelegramService,
    token: &str,
    offset: i64,
) -> Result<Vec<TelegramUpdate>, TelegramError> {
    request_json(
        service,
        token,
        "getUpdates",
        &GetUpdatesRequest {
            offset,
            timeout: POLL_TIMEOUT_SECONDS,
            allowed_updates: ["message", "callback_query", "inline_query"],
        },
    )
    .await
}

async fn handle_update(
    service: &TelegramService,
    token: &str,
    update: TelegramUpdate,
) -> Result<(), TelegramError> {
    if let Some(inline_query) = update.inline_query {
        return handle_inline_query(service, token, inline_query).await;
    }
    if let Some(callback_query) = update.callback_query {
        return handle_callback_query(service, token, callback_query).await;
    }
    let Some(message) = update.message else {
        return Ok(());
    };
    let chat_id = message.chat.id.to_string();
    let is_private = message.chat.chat_type == "private";
    if is_private && let Some(contact) = message.contact.as_ref() {
        return handle_contact(service, token, &message, contact).await;
    }
    let Some(text) = message.text.as_deref() else {
        return Ok(());
    };
    if is_private && handle_private_text(service, token, &message, text).await? {
        return Ok(());
    }
    let Some((command, argument)) = parse_command(text) else {
        return Ok(());
    };

    match command.as_str() {
        "start" if is_private => {
            let Some(user) = message.from.as_ref() else {
                return Ok(());
            };
            if argument.is_empty() {
                send_message(
                    service,
                    token,
                    &chat_id,
                    "Assalomu alaykum! Accord botga xush kelibsiz. Admin yuborgan invite link orqali qayta Start bosing.",
                    None,
                )
                .await?;
                return Ok(());
            }
            let account = service
                .register_start(TelegramStartRequest {
                    invite_token: argument,
                    telegram_user_id: user.id.to_string(),
                    telegram_chat_id: chat_id.clone(),
                    username: user.username.clone().unwrap_or_default(),
                    display_name: telegram_display_name(user),
                })
                .await;
            match account {
                Ok(account) => {
                    let text = role_guide(account.role);
                    send_message_with_markup(
                        service,
                        token,
                        &chat_id,
                        &text,
                        None,
                        role_guide_keyboard(account.role),
                    )
                    .await?;
                }
                Err(error) => {
                    let text = format!("Ulanish amalga oshmadi: {}", start_error_message(&error));
                    send_message(service, token, &chat_id, &text, None).await?;
                }
            }
        }
        "help" | "commands" => {
            let text = match message.from.as_ref() {
                Some(user) => match service.user_by_telegram_id(&user.id.to_string()).await? {
                    Some(account) => {
                        let text = role_guide(account.role);
                        send_message_with_markup(
                            service,
                            token,
                            &chat_id,
                            &text,
                            message.message_thread_id,
                            if is_private {
                                role_guide_keyboard(account.role)
                            } else {
                                None
                            },
                        )
                        .await?;
                        return Ok(());
                    }
                    None => general_guide().to_string(),
                },
                None => general_guide().to_string(),
            };
            send_message(service, token, &chat_id, &text, message.message_thread_id).await?;
        }
        "start" if !is_private && argument == "connect" => {
            connect_group(service, token, &message).await?;
        }
        "connect" if !is_private => {
            connect_group(service, token, &message).await?;
        }
        _ => {}
    }
    Ok(())
}

async fn handle_inline_query(
    service: &TelegramService,
    token: &str,
    inline_query: TelegramInlineQuery,
) -> Result<(), TelegramError> {
    answer_inline_query(service, token, &inline_query.id).await?;

    let telegram_user_id = inline_query.from.id.to_string();
    let Some(account) = service.user_by_telegram_id(&telegram_user_id).await? else {
        return Ok(());
    };
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
            send_message_with_markup(
                service,
                token,
                &chat_id,
                "Login jarayoni bekor qilindi.",
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
        send_photo(service, &token, chat, notification, image).await
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
    let file_name = if image.image_name.trim().is_empty() {
        "order-image.jpg".to_string()
    } else {
        image.image_name.trim().to_string()
    };
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
) -> Result<(), TelegramError> {
    let body = serde_json::json!({
        "inline_query_id": inline_query_id,
        "results": [],
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

async fn request_json<T: for<'de> Deserialize<'de> + Default, P: Serialize>(
    service: &TelegramService,
    token: &str,
    method: &str,
    payload: &P,
) -> Result<T, TelegramError> {
    let response = service
        .http_client()
        .post(bot_url(token, method))
        .json(payload)
        .send()
        .await
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
    parse_api_response(response).await
}

async fn parse_api_response<T: for<'de> Deserialize<'de> + Default>(
    response: reqwest::Response,
) -> Result<T, TelegramError> {
    let status = response.status();
    let payload = response
        .json::<TelegramApiResponse<T>>()
        .await
        .map_err(|error| TelegramError::Transport(error.to_string()))?;
    if !status.is_success() || !payload.ok {
        return Err(TelegramError::Transport(
            payload
                .description
                .unwrap_or_else(|| format!("telegram api http status {status}")),
        ));
    }
    payload
        .result
        .ok_or_else(|| TelegramError::Transport("telegram api returned no result".to_string()))
}

fn parse_command(text: &str) -> Option<(String, String)> {
    let mut parts = text.trim().splitn(2, char::is_whitespace);
    let command = parts.next()?.strip_prefix('/')?;
    let command = command.split('@').next()?.to_ascii_lowercase();
    Some((command, parts.next().unwrap_or_default().trim().to_string()))
}

#[derive(Debug, PartialEq, Eq)]
enum InlineLoginInput {
    Code(String),
    Password(String),
}

fn parse_inline_login_input(query: &str) -> Option<InlineLoginInput> {
    let mut parts = query.trim().splitn(2, char::is_whitespace);
    let prefix = parts.next()?.to_ascii_lowercase();
    let value = parts.next()?.trim();
    if value.is_empty() {
        return None;
    }
    match prefix.as_str() {
        "q7" if is_login_code(value) => Some(InlineLoginInput::Code(value.to_string())),
        "p4" => Some(InlineLoginInput::Password(value.to_string())),
        _ => None,
    }
}

fn is_login_code(value: &str) -> bool {
    let value = value.trim();
    (4..=8).contains(&value.len()) && value.chars().all(|character| character.is_ascii_digit())
}

fn user_account_error_message(error: &TelegramError) -> String {
    match error {
        TelegramError::UserAccountNotConfigured => {
            "User profile ulash sozlanmagan: serverda TELEGRAM_API_ID va TELEGRAM_API_HASH kerak."
                .to_string()
        }
        TelegramError::UserAccountNotAuthorized => {
            "User profile ulanmagan yoki login jarayoni tugamagan.".to_string()
        }
        TelegramError::UserAccountInvalidCode => {
            "Login kodi noto‘g‘ri yoki muddati tugagan. Qayta yuboring.".to_string()
        }
        TelegramError::UserAccountSignUpRequired => {
            "Bu raqam Telegram’da ro‘yxatdan o‘tmagan. Avval Telegram ilovasida account oching."
                .to_string()
        }
        TelegramError::UserAccountAccountMismatch => {
            "Yuborilgan raqam botga kirgan Telegram profilingizga tegishli emas.".to_string()
        }
        TelegramError::UserAccountGroupNotWritable => {
            "Tanlangan guruhga user profile yozolmaydi yoki guruh endi mavjud emas.".to_string()
        }
        TelegramError::UserAccount(error) => format!("User profile ulanmadi: {error}"),
        _ => format!("Telegram user profile xatosi: {error}"),
    }
}

fn telegram_display_name(user: &TelegramUser) -> String {
    let name = format!("{} {}", user.first_name.trim(), user.last_name.trim());
    let name = name.trim();
    if !name.is_empty() {
        name.to_string()
    } else {
        user.username
            .as_deref()
            .unwrap_or("Telegram user")
            .to_string()
    }
}

fn start_error_message(error: &TelegramError) -> &'static str {
    match error {
        TelegramError::InviteExpired => "invite link muddati tugagan",
        TelegramError::InviteAlreadyUsed => "invite link boshqa user tomonidan ishlatilgan",
        TelegramError::InviteNotFound => "invite link topilmadi",
        TelegramError::InviteTokenRequired => "invite token berilmagan",
        _ => "server xatosi",
    }
}

fn general_guide() -> &'static str {
    r#"Assalomu alaykum! Accord botdan foydalanish uchun admin yuborgan invite linkni ochib, /start tugmasini bosing.

📘 Commandlar:
/help — bot imkoniyatlari va role qo‘llanmasini ko‘rsatadi.
/start — invite link orqali tizimga kirish.
/connect — bot turgan guruhni orderlar uchun ulash; bu command guruh ichida yuboriladi."#
}

fn role_guide(role: TelegramAccountRole) -> String {
    let role_label = role.label();
    let role_details = match role {
        TelegramAccountRole::Admin => {
            r#"🛠 Admin vazifalari:
• Mobile ilovadagi Telegram bo‘limidan bot sozlamalarini boshqarish.
• Admin va sotuv managerlari uchun invite link yaratish.
• Botni order guruhiga qo‘shib, guruhda /connect yuborish."#
        }
        TelegramAccountRole::SalesManager => {
            r#"💼 Sotuv manageri vazifalari:
• Mijozlar va orderlar bilan ishlash.
• Yangi orderlar yuborilgan guruhni kuzatish.
• Order tafsilotlarini ishlab chiqarish jamoasiga yetkazish."#
        }
    };
    let role_commands = match role {
        TelegramAccountRole::Admin => {
            r#"🛠 Admin commandlari:
/help yoki /commands — admin qo‘llanmasi.
/connect — bot turgan guruhni orderlar uchun ulash; guruh ichida yuboriladi."#
        }
        TelegramAccountRole::SalesManager => {
            r#"💼 Sotuv manageri commandlari:
/bot_mode — orderni bot orqali yuborish.
/user_mode — o‘z Telegram profilingiz orqali yuborish va guruh tanlash.
/groups — user profile ulangan bo‘lsa yozish mumkin guruhlarni chiqarish.
/code yoki /password — login kod/parol uchun inline yuborish tugmasini chiqarish.
/cancel — joriy login jarayonini bekor qilish."#
        }
    };
    format!(
        r#"✅ Ulanish muvaffaqiyatli!
Sizning rolingiz: {role_label}

📘 {role_label} qo‘llanmasi
{role_details}

🤖 Commandlar:
/help — {role_label} qo‘llanmasini qayta ko‘rsatadi.
/commands — commandlar ro‘yxatini qayta chiqaradi.
/start — invite link orqali tizimga kirish yoki role ulanishini yangilash.
/connect — guruhni orderlar uchun ulash; guruh ichida yuboriladi.

{role_commands}

📦 Orderlar
Yangi orderlar tanlangan delivery mode bo‘yicha yuboriladi."#
    )
}

fn bot_url(token: &str, method: &str) -> String {
    format!("{TELEGRAM_API_BASE}{token}/{method}")
}

fn truncate_caption(value: &str) -> String {
    value.chars().take(1000).collect()
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("—")
        .to_string()
}

fn empty_dash(value: &str) -> &str {
    if value.trim().is_empty() {
        "—"
    } else {
        value.trim()
    }
}

fn number_or_dash(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        return "—".to_string();
    }
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InlineLoginInput, TelegramOrderNotification, is_login_code, number_or_dash, parse_command,
        parse_inline_login_input, role_guide,
    };
    use crate::core::calculate_orders::CalculateOrderTemplate;
    use crate::core::production_map::ProductionMapDefinition;
    use crate::telegram::TelegramAccountRole;

    #[test]
    fn parse_command_accepts_bot_mention_and_start_parameter() {
        assert_eq!(
            parse_command("/start@accord_bot invite123"),
            Some(("start".to_string(), "invite123".to_string()))
        );
        assert_eq!(
            parse_command("/code 12345"),
            Some(("code".to_string(), "12345".to_string()))
        );
        assert!(is_login_code("12345"));
        assert!(!is_login_code("12 345"));
    }

    #[test]
    fn role_guide_explains_role_and_commands() {
        let admin_guide = role_guide(TelegramAccountRole::Admin);
        assert!(admin_guide.contains("Sizning rolingiz: Admin"));
        assert!(admin_guide.contains("invite link yaratish"));
        assert!(admin_guide.contains("/help"));
        assert!(admin_guide.contains("/connect"));
        assert!(!admin_guide.contains("/user_mode"));

        let manager_guide = role_guide(TelegramAccountRole::SalesManager);
        assert!(manager_guide.contains("Sizning rolingiz: Sotuv manageri"));
        assert!(manager_guide.contains("Yangi orderlar yuborilgan guruhni kuzatish"));
        assert!(manager_guide.contains("/commands"));
        assert!(manager_guide.contains("/user_mode"));
        assert!(manager_guide.contains("inline"));
        assert!(!manager_guide.contains("/password <parol>"));
    }

    #[test]
    fn inline_login_input_keeps_code_and_password_out_of_normal_messages() {
        assert_eq!(
            parse_inline_login_input("q7 47989"),
            Some(InlineLoginInput::Code("47989".to_string()))
        );
        assert_eq!(
            parse_inline_login_input("p4 my secret password"),
            Some(InlineLoginInput::Password("my secret password".to_string()))
        );
        assert_eq!(parse_inline_login_input("47989"), None);
        assert_eq!(parse_inline_login_input("q7 12"), None);
    }

    #[test]
    fn order_notification_contains_core_order_fields() {
        let notification = TelegramOrderNotification::from_order(
            ProductionMapDefinition {
                id: "zakaz-2731".to_string(),
                product_code: "MOLLY".to_string(),
                title: "Molly".to_string(),
                code: "2731".to_string(),
                order_number: "2731".to_string(),
                customer_name: "Freshboll".to_string(),
                roll_count: None,
                width_mm: Some(680.0),
                order_kg: Some(1000.0),
                base_length: None,
                nodes: Vec::new(),
                edges: Vec::new(),
            },
            CalculateOrderTemplate {
                name: "Molly order".to_string(),
                product: "Molly 70 gr Sour Pencil mix".to_string(),
                material_display: "PET 12 + CPP 35".to_string(),
                color: "faylga".to_string(),
                kg: 1000.0,
                frame_product_size_mm: 220.0,
                frame_count: 3.0,
                ..CalculateOrderTemplate::default()
            },
            None,
            "Valiyev Abdulla".to_string(),
        );
        assert!(notification.caption.contains("№2731"));
        assert!(notification.caption.contains("Freshboll"));
        assert!(notification.caption.contains("Valiyev Abdulla"));
    }

    #[test]
    fn number_format_is_compact() {
        assert_eq!(number_or_dash(1000.0), "1000");
        assert_eq!(number_or_dash(12.5), "12.5");
        assert_eq!(number_or_dash(0.0), "—");
    }
}
