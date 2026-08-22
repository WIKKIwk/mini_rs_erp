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
use super::order::{TelegramOrderDraft, TelegramOrderLayer, TelegramOrderStep, order_caption};
use super::service::{TelegramError, TelegramService};
use super::useraccount::{CodeOutcome, LoginOutcome};

const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";
const POLL_TIMEOUT_SECONDS: u64 = 25;
const INLINE_CODE_PREFIX: &str = "q7 ";
const INLINE_PASSWORD_PREFIX: &str = "p4 ";
const INLINE_CUSTOMER_PREFIX: &str = "c7 ";
const INLINE_PRODUCT_PREFIX: &str = "i7 ";
const INLINE_MATERIAL_PREFIX: &str = "m7 ";
const INLINE_MICRON_PREFIX: &str = "n7 ";
const MAX_ORDER_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const TELEGRAM_FILE_BASE: &str = "https://api.telegram.org/file/bot";

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
    photo: Option<Vec<TelegramPhotoSize>>,
    #[serde(default)]
    document: Option<TelegramDocument>,
    #[serde(default)]
    contact: Option<TelegramContact>,
    #[serde(default)]
    message_thread_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct TelegramPhotoSize {
    file_id: String,
    width: i64,
    height: i64,
    #[serde(default)]
    file_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TelegramDocument {
    file_id: String,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    file_size: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct TelegramFile {
    #[serde(default)]
    file_path: Option<String>,
}

#[derive(Debug)]
struct TelegramOrderMedia {
    file_id: String,
    file_name: String,
    mime_type: String,
    file_size: Option<u64>,
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

include!("parts/part_01.rs");
include!("parts/part_02.rs");
include!("parts/part_03.rs");
include!("parts/part_04.rs");
include!("parts/part_05.rs");
include!("parts/part_06.rs");
include!("parts/part_07.rs");
include!("inline_tests.rs");
