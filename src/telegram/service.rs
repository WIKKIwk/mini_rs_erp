use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use base64::Engine;
use time::OffsetDateTime;

use crate::core::calculate_orders::{CalculateOrderImage, CalculateOrderTemplate};
use crate::core::production_map::ProductionMapDefinition;

use super::models::{
    TelegramAdminOverview, TelegramBotSettings, TelegramBotSettingsUpdate, TelegramChat,
    TelegramDeliveryMode, TelegramInviteRequest, TelegramInviteResponse, TelegramStartRequest,
    TelegramUserAccount, TelegramUserGroup,
};
use super::order::TelegramOrderDraft;
use super::order_catalog::TelegramOrderCatalog;
use super::store::{TelegramStore, TelegramStoreError};
use super::useraccount::{CodeOutcome, LoginOutcome, TelegramUserAccountService, UserAccountError};

#[derive(Debug, thiserror::Error)]
pub enum TelegramError {
    #[error("telegram bot username is required")]
    BotUsernameRequired,
    #[error("telegram bot token is not configured")]
    BotTokenRequired,
    #[error("telegram invite token is required")]
    InviteTokenRequired,
    #[error("telegram user id is required")]
    UserIdRequired,
    #[error("telegram invite not found")]
    InviteNotFound,
    #[error("telegram invite already used")]
    InviteAlreadyUsed,
    #[error("telegram invite expired")]
    InviteExpired,
    #[error("telegram transport failed: {0}")]
    Transport(String),
    #[error("telegram user account API credentials are not configured")]
    UserAccountNotConfigured,
    #[error("telegram user account is not connected")]
    UserAccountNotAuthorized,
    #[error("telegram login code is invalid or expired")]
    UserAccountInvalidCode,
    #[error("telegram account registration is required")]
    UserAccountSignUpRequired,
    #[error("telegram account does not match the bot user")]
    UserAccountAccountMismatch,
    #[error("telegram selected group is not writable")]
    UserAccountGroupNotWritable,
    #[error("telegram user account operation failed: {0}")]
    UserAccount(String),
    #[error("telegram store failed")]
    Store,
    #[error("telegram order catalog is not configured")]
    OrderCatalogNotConfigured,
    #[error("telegram order catalog failed: {0}")]
    OrderCatalog(String),
}

#[derive(Clone)]
pub struct TelegramService {
    store: Arc<TelegramStore>,
    useraccount: TelegramUserAccountService,
    http: reqwest::Client,
    worker_started: Arc<AtomicBool>,
    order_catalog: Option<Arc<TelegramOrderCatalog>>,
    order_choices: Arc<tokio::sync::Mutex<BTreeMap<String, String>>>,
}

impl TelegramService {
    pub fn new(path: PathBuf) -> Self {
        let store = Arc::new(TelegramStore::new(path));
        Self {
            useraccount: TelegramUserAccountService::new(store.clone()),
            store,
            http: reqwest::Client::new(),
            worker_started: Arc::new(AtomicBool::new(false)),
            order_catalog: None,
            order_choices: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        }
    }

    pub fn with_order_catalog(
        mut self,
        admin: crate::core::admin::service::AdminService,
        materials: Arc<dyn crate::core::calculate_materials::CalculateMaterialStorePort>,
        production_maps: crate::core::production_map::ProductionMapService,
    ) -> Self {
        self.order_catalog = Some(Arc::new(TelegramOrderCatalog::new(
            admin,
            materials,
            production_maps,
        )));
        self
    }

    pub async fn admin_overview(&self) -> Result<TelegramAdminOverview, TelegramError> {
        let (bot_username, bot_token) = self.store.bot_settings().await.map_err(map_store)?;
        let mut users = self.store.users().await.map_err(map_store)?;
        let mut chats = self.store.chats().await.map_err(map_store)?;
        users.sort_by(|left, right| {
            right
                .joined_at_unix
                .cmp(&left.joined_at_unix)
                .then_with(|| left.display_name.cmp(&right.display_name))
        });
        chats.sort_by(|left, right| {
            right
                .last_seen_at_unix
                .cmp(&left.last_seen_at_unix)
                .then_with(|| left.title.cmp(&right.title))
        });
        Ok(TelegramAdminOverview {
            bot: TelegramBotSettings {
                bot_username,
                token_configured: !bot_token.trim().is_empty(),
                token_hint: token_hint(&bot_token),
            },
            users,
            chats,
        })
    }

    pub async fn update_bot_settings(
        &self,
        input: TelegramBotSettingsUpdate,
    ) -> Result<TelegramAdminOverview, TelegramError> {
        let bot_username = normalize_bot_username(&input.bot_username);
        let (_, current_token) = self.store.bot_settings().await.map_err(map_store)?;
        let bot_token = if input.bot_token.trim().is_empty() {
            None
        } else {
            Some(input.bot_token.trim().to_string())
        };
        self.store
            .set_bot_settings(bot_username, bot_token.or(Some(current_token)))
            .await
            .map_err(map_store)?;
        self.start_bot_worker_if_configured().await;
        self.admin_overview().await
    }

    pub async fn start_bot_worker_if_configured(&self) {
        if cfg!(test) {
            return;
        }
        let Ok((_, token)) = self.store.bot_settings().await else {
            return;
        };
        if token.trim().is_empty() || self.worker_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let service = self.clone();
        tokio::spawn(async move {
            super::bot::run_polling(service).await;
        });
    }

    pub async fn create_invite(
        &self,
        input: TelegramInviteRequest,
    ) -> Result<TelegramInviteResponse, TelegramError> {
        let (bot_username, bot_token) = self.store.bot_settings().await.map_err(map_store)?;
        if bot_username.is_empty() {
            return Err(TelegramError::BotUsernameRequired);
        }
        if bot_token.trim().is_empty() {
            return Err(TelegramError::BotTokenRequired);
        }
        let token = create_invite_token();
        self.store
            .create_invite(
                token.clone(),
                input.role,
                OffsetDateTime::now_utc().unix_timestamp(),
            )
            .await
            .map_err(map_store)?;
        Ok(TelegramInviteResponse {
            role: input.role,
            invite_url: format!("https://t.me/{bot_username}?start={token}"),
        })
    }

    pub async fn register_start(
        &self,
        input: TelegramStartRequest,
    ) -> Result<TelegramUserAccount, TelegramError> {
        let invite_token = input.invite_token.trim();
        if invite_token.is_empty() {
            return Err(TelegramError::InviteTokenRequired);
        }
        let telegram_user_id = input.telegram_user_id.trim();
        if telegram_user_id.is_empty() {
            return Err(TelegramError::UserIdRequired);
        }
        let display_name = if input.display_name.trim().is_empty() {
            input.username.trim().to_string()
        } else {
            input.display_name.trim().to_string()
        };
        self.store
            .claim_invite(
                invite_token,
                telegram_user_id.to_string(),
                input.telegram_chat_id.trim().to_string(),
                input.username.trim().trim_start_matches('@').to_string(),
                display_name,
                OffsetDateTime::now_utc().unix_timestamp(),
            )
            .await
            .map_err(map_store)
    }

    pub(crate) async fn connect_chat(&self, chat: TelegramChat) -> Result<(), TelegramError> {
        self.store.upsert_chat(chat).await.map_err(map_store)
    }

    pub(crate) async fn user_by_telegram_id(
        &self,
        telegram_user_id: &str,
    ) -> Result<Option<TelegramUserAccount>, TelegramError> {
        self.store
            .user_by_telegram_id(telegram_user_id)
            .await
            .map_err(map_store)
    }

    pub(crate) async fn user_by_phone(
        &self,
        phone_number: &str,
    ) -> Result<Option<TelegramUserAccount>, TelegramError> {
        self.store
            .user_by_phone(phone_number)
            .await
            .map_err(map_store)
    }

    pub(crate) async fn set_delivery_mode(
        &self,
        telegram_user_id: &str,
        delivery_mode: TelegramDeliveryMode,
    ) -> Result<TelegramUserAccount, TelegramError> {
        self.useraccount
            .set_delivery_mode(telegram_user_id, delivery_mode)
            .await
            .map_err(map_user_account)
    }

    pub(crate) async fn begin_user_profile_login(
        &self,
        telegram_user_id: &str,
        phone_number: &str,
    ) -> Result<LoginOutcome, TelegramError> {
        self.useraccount
            .begin_login(telegram_user_id, phone_number)
            .await
            .map_err(map_user_account)
    }

    pub(crate) async fn complete_user_profile_code(
        &self,
        telegram_user_id: &str,
        code: &str,
    ) -> Result<CodeOutcome, TelegramError> {
        self.useraccount
            .complete_code(telegram_user_id, code)
            .await
            .map_err(map_user_account)
    }

    pub(crate) async fn complete_user_profile_password(
        &self,
        telegram_user_id: &str,
        password: &str,
    ) -> Result<TelegramUserAccount, TelegramError> {
        self.useraccount
            .complete_password(telegram_user_id, password)
            .await
            .map_err(map_user_account)
    }

    pub(crate) async fn has_pending_user_profile_login(&self, telegram_user_id: &str) -> bool {
        self.useraccount.has_pending_login(telegram_user_id).await
    }

    pub(crate) async fn cancel_user_profile_login(&self, telegram_user_id: &str) {
        self.useraccount.cancel_login(telegram_user_id).await;
    }

    pub(crate) async fn writable_user_groups(
        &self,
        telegram_user_id: &str,
    ) -> Result<Vec<TelegramUserGroup>, TelegramError> {
        self.useraccount
            .writable_groups(telegram_user_id)
            .await
            .map_err(map_user_account)
    }

    pub(crate) async fn select_user_group(
        &self,
        telegram_user_id: &str,
        chat_id: &str,
        chat_type: &str,
    ) -> Result<TelegramUserAccount, TelegramError> {
        self.useraccount
            .select_group(telegram_user_id, chat_id, chat_type)
            .await
            .map_err(map_user_account)
    }

    pub(crate) async fn send_order_to_user_profile(
        &self,
        telegram_user_id: &str,
        caption: &str,
    ) -> Result<(), TelegramError> {
        self.useraccount
            .send_text_to_selected_group(telegram_user_id, caption)
            .await
            .map_err(map_user_account)
    }

    pub(crate) async fn order_catalog(&self) -> Result<Arc<TelegramOrderCatalog>, TelegramError> {
        self.order_catalog
            .clone()
            .ok_or(TelegramError::OrderCatalogNotConfigured)
    }

    pub(crate) async fn order_draft(
        &self,
        telegram_user_id: &str,
    ) -> Result<Option<TelegramOrderDraft>, TelegramError> {
        self.store
            .order_draft(telegram_user_id)
            .await
            .map_err(map_store)
    }

    pub(crate) async fn save_order_draft(
        &self,
        telegram_user_id: &str,
        draft: TelegramOrderDraft,
    ) -> Result<(), TelegramError> {
        self.store
            .save_order_draft(telegram_user_id, draft)
            .await
            .map_err(map_store)
    }

    pub(crate) async fn clear_order_draft(
        &self,
        telegram_user_id: &str,
    ) -> Result<(), TelegramError> {
        self.store
            .clear_order_draft(telegram_user_id)
            .await
            .map_err(map_store)
    }

    pub(crate) async fn remember_order_choice(
        &self,
        telegram_user_id: &str,
        value: String,
    ) -> String {
        let token = format!("{:016x}", rand::random::<u64>());
        let key = format!("{telegram_user_id}:{token}");
        let mut choices = self.order_choices.lock().await;
        choices.insert(key, value);
        while choices.len() > 4096 {
            let Some(first) = choices.keys().next().cloned() else {
                break;
            };
            choices.remove(&first);
        }
        token
    }

    pub(crate) async fn take_order_choice(
        &self,
        telegram_user_id: &str,
        token: &str,
    ) -> Option<String> {
        self.order_choices
            .lock()
            .await
            .remove(&format!("{telegram_user_id}:{token}"))
    }

    pub(crate) async fn deliver_order_caption(
        &self,
        telegram_user_id: &str,
        caption: &str,
    ) -> Result<usize, TelegramError> {
        let Some(account) = self.user_by_telegram_id(telegram_user_id).await? else {
            return Err(TelegramError::UserAccountNotAuthorized);
        };
        if account.delivery_mode == TelegramDeliveryMode::UserProfile {
            self.send_order_to_user_profile(telegram_user_id, caption)
                .await?;
            return Ok(1);
        }
        let chats = self.store.chats().await.map_err(map_store)?;
        let mut delivered = 0;
        let mut last_error = None;
        for chat in chats {
            match super::bot::send_text_to_chat(self, &chat, caption).await {
                Ok(()) => delivered += 1,
                Err(error) => {
                    tracing::warn!(chat_id = %chat.chat_id, ?error, "telegram order delivery failed");
                    last_error = Some(error);
                }
            }
        }
        if delivered == 0
            && let Some(error) = last_error
        {
            return Err(error);
        }
        Ok(delivered)
    }

    pub(crate) async fn update_offset(&self) -> Result<i64, TelegramError> {
        self.store.update_offset().await.map_err(map_store)
    }

    pub(crate) async fn set_update_offset(&self, offset: i64) -> Result<(), TelegramError> {
        self.store
            .set_update_offset(offset)
            .await
            .map_err(map_store)
    }

    pub(crate) async fn bot_credentials(&self) -> Result<(String, String), TelegramError> {
        self.store.bot_settings().await.map_err(map_store)
    }

    pub(crate) fn http_client(&self) -> reqwest::Client {
        self.http.clone()
    }

    pub async fn notify_order_created(
        &self,
        map: ProductionMapDefinition,
        template: CalculateOrderTemplate,
        image: Option<CalculateOrderImage>,
        sender_display_name: String,
        sender_phone: String,
    ) -> Result<usize, TelegramError> {
        let notification = super::bot::TelegramOrderNotification::from_order(
            map,
            template,
            image,
            sender_display_name,
        );
        if let Some(account) = self.user_by_phone(&sender_phone).await?
            && account.delivery_mode == TelegramDeliveryMode::UserProfile
        {
            self.send_order_to_user_profile(&account.telegram_user_id, &notification.caption)
                .await?;
            return Ok(1);
        }
        let chats = self.store.chats().await.map_err(map_store)?;
        if chats.is_empty() {
            return Ok(0);
        }
        let mut delivered = 0;
        let mut last_error = None;
        for chat in chats {
            match super::bot::send_order_to_chat(self, &chat, &notification).await {
                Ok(()) => delivered += 1,
                Err(error) => {
                    tracing::warn!(
                        chat_id = %chat.chat_id,
                        ?error,
                        "telegram order delivery failed"
                    );
                    last_error = Some(error);
                }
            }
        }
        if delivered == 0 {
            if let Some(error) = last_error {
                return Err(error);
            }
        }
        Ok(delivered)
    }
}

fn map_user_account(error: UserAccountError) -> TelegramError {
    match error {
        UserAccountError::NotConfigured => TelegramError::UserAccountNotConfigured,
        UserAccountError::NotAuthorized | UserAccountError::LoginNotPending => {
            TelegramError::UserAccountNotAuthorized
        }
        UserAccountError::InvalidCode => TelegramError::UserAccountInvalidCode,
        UserAccountError::SignUpRequired => TelegramError::UserAccountSignUpRequired,
        UserAccountError::AccountMismatch => TelegramError::UserAccountAccountMismatch,
        UserAccountError::GroupNotWritable => TelegramError::UserAccountGroupNotWritable,
        UserAccountError::Transport(error) | UserAccountError::Store(error) => {
            TelegramError::UserAccount(error)
        }
    }
}

fn map_store(error: TelegramStoreError) -> TelegramError {
    match error {
        TelegramStoreError::InviteNotFound => TelegramError::InviteNotFound,
        TelegramStoreError::InviteAlreadyUsed => TelegramError::InviteAlreadyUsed,
        TelegramStoreError::InviteExpired => TelegramError::InviteExpired,
        TelegramStoreError::UserNotFound => TelegramError::UserAccountNotAuthorized,
        TelegramStoreError::Read
        | TelegramStoreError::Write
        | TelegramStoreError::SessionCrypto => TelegramError::Store,
    }
}

fn normalize_bot_username(value: &str) -> String {
    let mut value = value.trim().trim_start_matches('@').trim_end_matches('/');
    for prefix in ["https://t.me/", "http://t.me/", "t.me/"] {
        if let Some(rest) = value.strip_prefix(prefix) {
            value = rest.trim_start_matches('@').trim_end_matches('/');
            break;
        }
    }
    value.to_string()
}

fn token_hint(token: &str) -> String {
    let token = token.trim();
    if token.is_empty() {
        return String::new();
    }
    if token.len() <= 8 {
        return "••••••••".to_string();
    }
    format!("••••{}", &token[token.len() - 4..])
}

fn create_invite_token() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 18]>())
}

#[cfg(test)]
mod tests {
    use super::{TelegramService, normalize_bot_username};
    use crate::telegram::{
        TelegramAccountRole, TelegramBotSettingsUpdate, TelegramInviteRequest, TelegramStartRequest,
    };

    #[test]
    fn bot_username_is_normalized_for_deep_links() {
        assert_eq!(
            normalize_bot_username("https://t.me/@accord_bot/"),
            "accord_bot"
        );
    }

    #[tokio::test]
    async fn invite_can_be_claimed_once_and_returns_role() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = TelegramService::new(dir.path().join("telegram.json"));
        service
            .update_bot_settings(TelegramBotSettingsUpdate {
                bot_username: "accord_bot".to_string(),
                bot_token: "token".to_string(),
            })
            .await
            .expect("settings");
        let invite = service
            .create_invite(TelegramInviteRequest {
                role: TelegramAccountRole::SalesManager,
            })
            .await
            .expect("invite");
        let token = invite
            .invite_url
            .split("start=")
            .nth(1)
            .expect("invite token")
            .to_string();
        let user = service
            .register_start(TelegramStartRequest {
                invite_token: token,
                telegram_user_id: "123".to_string(),
                telegram_chat_id: "456".to_string(),
                username: "manager".to_string(),
                display_name: "Sales Manager".to_string(),
            })
            .await
            .expect("start");
        assert_eq!(user.role, TelegramAccountRole::SalesManager);
        assert_eq!(
            service
                .admin_overview()
                .await
                .expect("overview")
                .users
                .len(),
            1
        );
    }
}
