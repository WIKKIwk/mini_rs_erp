
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
    #[error("telegram is rate limiting login codes: retry after {seconds} seconds")]
    UserAccountFloodWait { seconds: u64 },
    #[error("telegram has no available delivery channel for login codes right now")]
    UserAccountSendCodeUnavailable,
    #[error("login code resend is too frequent: retry after {wait_seconds} seconds")]
    UserAccountResendTooSoon { wait_seconds: u64 },
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
    pub(crate) qr_logins: super::useraccount::qr::QrLoginService,
    http: reqwest::Client,
    worker_started: Arc<AtomicBool>,
    order_catalog: Option<Arc<TelegramOrderCatalog>>,
    pending_orders: Option<Arc<dyn crate::core::pending_orders::PendingOrderStore>>,
    order_choices: Arc<tokio::sync::Mutex<BTreeMap<String, String>>>,
}
