use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramAccountRole {
    Admin,
    SalesManager,
}

impl TelegramAccountRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Admin => "Admin",
            Self::SalesManager => "Sotuv manageri",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramDeliveryMode {
    #[default]
    Bot,
    UserProfile,
}

impl TelegramDeliveryMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bot => "Bot orqali",
            Self::UserProfile => "User profile orqali",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TelegramBotSettings {
    pub bot_username: String,
    pub token_configured: bool,
    pub token_hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramUserAccount {
    pub telegram_user_id: String,
    #[serde(default)]
    pub telegram_chat_id: String,
    pub username: String,
    pub display_name: String,
    pub role: TelegramAccountRole,
    pub invite_token: String,
    pub joined_at_unix: i64,
    #[serde(default)]
    pub phone_number: String,
    #[serde(default)]
    pub delivery_mode: TelegramDeliveryMode,
    #[serde(default)]
    pub user_profile_connected: bool,
    #[serde(default)]
    pub selected_chat_id: Option<String>,
    #[serde(default)]
    pub selected_chat_title: Option<String>,
    #[serde(default)]
    pub selected_chat_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramUserGroup {
    pub chat_id: String,
    pub title: String,
    pub chat_type: String,
    #[serde(default)]
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramChat {
    pub chat_id: String,
    pub title: String,
    #[serde(default)]
    pub username: String,
    pub chat_type: String,
    #[serde(default)]
    pub thread_id: Option<i64>,
    pub connected_at_unix: i64,
    pub last_seen_at_unix: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TelegramAdminOverview {
    pub bot: TelegramBotSettings,
    pub users: Vec<TelegramUserAccount>,
    pub chats: Vec<TelegramChat>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct TelegramBotSettingsUpdate {
    pub bot_username: String,
    #[serde(default)]
    pub bot_token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct TelegramInviteRequest {
    pub role: TelegramAccountRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TelegramInviteResponse {
    pub role: TelegramAccountRole,
    pub invite_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct TelegramStartRequest {
    pub invite_token: String,
    pub telegram_user_id: String,
    #[serde(default)]
    pub telegram_chat_id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub display_name: String,
}
