//! Telegram integration boundary for optional bot-backed workflows.
//!
//! The module owns Telegram-specific state and contracts. It does not start a
//! polling or webhook worker by itself, so Mini RS ERP remains fully usable
//! when Telegram is not configured.

pub mod bot;
mod models;
mod order;
mod order_catalog;
mod service;
mod store;
pub mod useraccount;

pub use models::{
    TelegramAccountRole, TelegramAdminOverview, TelegramBotSettingsUpdate, TelegramChat,
    TelegramDeliveryMode, TelegramInviteRequest, TelegramInviteResponse, TelegramStartRequest,
    TelegramUserAccount, TelegramUserGroup,
};
pub use service::{TelegramError, TelegramService};
