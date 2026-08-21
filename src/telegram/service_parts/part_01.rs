
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
