
#[derive(Debug, thiserror::Error)]
pub(crate) enum UserAccountError {
    #[error("telegram user account API credentials are not configured")]
    NotConfigured,
    #[error("telegram user account is not connected")]
    NotAuthorized,
    #[error("telegram user account login is not pending")]
    LoginNotPending,
    #[error("telegram login code is invalid or expired")]
    InvalidCode,
    #[error("telegram is rate limiting login codes: retry after {seconds} seconds")]
    FloodWait { seconds: u64 },
    #[error("telegram has no available delivery channel for login codes right now")]
    SendCodeUnavailable,
    #[error("login code resend is too frequent: retry after {wait_seconds} seconds")]
    ResendTooSoon { wait_seconds: u64 },
    #[error("telegram account registration is required")]
    SignUpRequired,
    #[error("telegram account does not match the bot user")]
    AccountMismatch,
    #[error("telegram group is not writable by this account")]
    GroupNotWritable,
    #[error("telegram user session failed: {0}")]
    Transport(String),
    #[error("telegram user session store failed: {0}")]
    Store(String),
}

#[derive(Debug)]
pub(crate) enum LoginOutcome {
    CodeSent,
    Authorized,
}

#[derive(Debug)]
pub(crate) enum ResendOutcome {
    Resent,
    Authorized,
}

#[derive(Debug)]
pub(crate) enum CodeOutcome {
    PasswordRequired { hint: Option<String> },
    Authorized,
}

struct PendingLogin {
    client: Client,
    shutdown: ShutdownToken,
    login_token: LoginToken,
    password_token: Option<PasswordToken>,
    phone_number: String,
    last_code_sent_at: std::time::Instant,
}

const RESEND_COOLDOWN_SECONDS: u64 = 60;

#[derive(Clone)]
pub(crate) struct TelegramUserAccountService {
    store: Arc<TelegramStore>,
    pending_logins: Arc<Mutex<BTreeMap<String, PendingLogin>>>,
    lookup_cache: Arc<lookup::LookupCache>,
}

impl TelegramUserAccountService {
    pub(crate) fn new(store: Arc<TelegramStore>) -> Self {
        Self {
            store,
            pending_logins: Arc::new(Mutex::new(BTreeMap::new())),
            lookup_cache: Arc::new(lookup::LookupCache::default()),
        }
    }

    pub(crate) async fn set_delivery_mode(
        &self,
        telegram_user_id: &str,
        delivery_mode: TelegramDeliveryMode,
    ) -> Result<TelegramUserAccount, UserAccountError> {
        let account = self.account(telegram_user_id).await?;
        if account.role != TelegramAccountRole::SalesManager {
            return Err(UserAccountError::Transport(
                "faqat sotuv manageri delivery mode tanlashi mumkin".to_string(),
            ));
        }
        self.store
            .set_delivery_mode(telegram_user_id, delivery_mode)
            .await
            .map_err(map_store)
    }

    pub(crate) async fn begin_login(
        &self,
        telegram_user_id: &str,
        phone_number: &str,
    ) -> Result<LoginOutcome, UserAccountError> {
        let _ = self.account(telegram_user_id).await?;
        let phone_number = normalize_phone(phone_number);
        if phone_number.is_empty() {
            return Err(UserAccountError::Transport(
                "telefon raqami noto‘g‘ri".to_string(),
            ));
        }
        let (api_id, api_hash) = api_credentials(&self.store).await?;
        let existing_session = self
            .store
            .user_session(telegram_user_id)
            .await
            .map_err(map_store)?;
        let (client, shutdown) =
            connect_client(api_id, &api_hash, existing_session.as_deref()).await?;

        if existing_session.is_some() && client.is_authorized().await.map_err(map_transport)? {
            return self
                .finish_login(telegram_user_id, phone_number, client, shutdown)
                .await
                .map(|_| LoginOutcome::Authorized);
        }

        // auth.sendCode uchun Telegram/MTProto'ning standart code_settings'ini
        // ishlatamiz. Telegram kodni contact'dagi raqamga bog'langan akkauntga
        // o'zi tanlagan xavfsiz kanal orqali yetkazadi (Telegram servis xabari,
        // SMS yoki qo'ng'iroq). Server userbot current_number/flash-call kabi
        // qurilmaga bog'liq hintlarni bera olmaydi va ular SMS'ni majburlamaydi.
        let code_request = client.request_login_code(&phone_number).await;
        match code_request {
            Ok(SendCodeOutcome::CodeRequired(login_token)) => {
                let pending = PendingLogin {
                    client,
                    shutdown,
                    login_token,
                    password_token: None,
                    phone_number,
                    last_code_sent_at: std::time::Instant::now(),
                };
                let mut logins = self.pending_logins.lock().await;
                if let Some(previous) = logins.insert(telegram_user_id.to_string(), pending) {
                    previous.shutdown.cancel();
                }
                tracing::info!(
                    telegram_user_id = %telegram_user_id,
                    "telegram login code requested"
                );
                Ok(LoginOutcome::CodeSent)
            }
            Ok(SendCodeOutcome::AlreadyAuthorized(_)) => self
                .finish_login(telegram_user_id, phone_number, client, shutdown)
                .await
                .map(|_| LoginOutcome::Authorized),
            Err(error) => {
                // FLOOD_WAIT alohida: user qayta-qayta bossa blok uzayadi,
                // shuning uchun kutish vaqtini aniq ko'rsatish shart.
                if let ferogram::ErrorKind::FloodWait(seconds) = error.kind() {
                    shutdown.cancel();
                    return Err(UserAccountError::FloodWait { seconds });
                }
                shutdown.cancel();
                Err(map_send_code_error(error))
            }
        }
    }

    pub(crate) async fn complete_code(
        &self,
        telegram_user_id: &str,
        code: &str,
    ) -> Result<CodeOutcome, UserAccountError> {
        let mut pending = self.take_pending(telegram_user_id).await?;
        let result = pending.client.sign_in(&pending.login_token, code).await;
        match result {
            Ok(_) => self
                .finish_login(
                    telegram_user_id,
                    pending.phone_number,
                    pending.client,
                    pending.shutdown,
                )
                .await
                .map(|_| CodeOutcome::Authorized),
            Err(SignInError::PasswordRequired(password_token)) => {
                let hint = password_token.hint().map(str::to_string);
                pending.password_token = Some(*password_token);
                self.put_pending(telegram_user_id, pending).await;
                Ok(CodeOutcome::PasswordRequired { hint })
            }
            Err(SignInError::InvalidCode) => {
                self.put_pending(telegram_user_id, pending).await;
                Err(UserAccountError::InvalidCode)
            }
            Err(SignInError::SignUpRequired) => {
                pending.shutdown.cancel();
                Err(UserAccountError::SignUpRequired)
            }
            Err(SignInError::Other(error)) => {
                self.put_pending(telegram_user_id, pending).await;
                Err(map_transport(error))
            }
        }
    }

    pub(crate) async fn complete_password(
        &self,
        telegram_user_id: &str,
        password: &str,
    ) -> Result<TelegramUserAccount, UserAccountError> {
        let pending = self.take_pending(telegram_user_id).await?;
        let Some(password_token) = pending.password_token.clone() else {
            self.put_pending(telegram_user_id, pending).await;
            return Err(UserAccountError::Transport(
                "2FA password bosqichi boshlanmagan".to_string(),
            ));
        };
        match pending
            .client
            .check_password(password_token, password)
            .await
        {
            Ok(_) => {
                self.finish_login(
                    telegram_user_id,
                    pending.phone_number,
                    pending.client,
                    pending.shutdown,
                )
                .await
            }
            Err(error) => {
                self.put_pending(telegram_user_id, pending).await;
                Err(map_transport(error))
            }
        }
    }

    pub(crate) async fn resend_code(
        &self,
        telegram_user_id: &str,
    ) -> Result<ResendOutcome, UserAccountError> {
        // Kod "yuborildi" lekin yetib kelmasa: Telegram keyingi yetkazish
        // usuliga o'tadi (ilovadagi xabar -> SMS -> qo'ng'iroq).
        let mut pending = self.take_pending(telegram_user_id).await?;
        // Har bosish eski kodni o'ldiradi + Telegram bloklaydi. Shuning uchun
        // ketma-ket bosishni server o'zi to'xtatadi (60 soniya cooldown).
        let elapsed = pending.last_code_sent_at.elapsed().as_secs();
        if elapsed < RESEND_COOLDOWN_SECONDS {
            let wait_seconds = RESEND_COOLDOWN_SECONDS - elapsed;
            self.put_pending(telegram_user_id, pending).await;
            return Err(UserAccountError::ResendTooSoon { wait_seconds });
        }
        // reason=None: bu maydon faqat rasmiy ilovalar uchun, begona qiymat
        // yuborish Telegram'da rad etilishi mumkin.
        match pending.client.resend_code(&pending.login_token, None).await {
            Ok(SendCodeOutcome::CodeRequired(login_token)) => {
                pending.login_token = login_token;
                pending.password_token = None;
                pending.last_code_sent_at = std::time::Instant::now();
                self.put_pending(telegram_user_id, pending).await;
                Ok(ResendOutcome::Resent)
            }
            Ok(SendCodeOutcome::AlreadyAuthorized(_)) => {
                self.finish_login(
                    telegram_user_id,
                    pending.phone_number,
                    pending.client,
                    pending.shutdown,
                )
                .await
            }
                .map(|_| ResendOutcome::Authorized),
            Err(error) => {
                // Flood bo'lsa ham pending saqlanadi: eski kod hali kelishi
                // mumkin, user uni kiritib ko'ra oladi.
                if let ferogram::ErrorKind::FloodWait(seconds) = error.kind() {
                    self.put_pending(telegram_user_id, pending).await;
                    return Err(UserAccountError::FloodWait { seconds });
                }
                let mapped = map_send_code_error(error);
                self.put_pending(telegram_user_id, pending).await;
                Err(mapped)
            }
        }
    }

    pub(crate) async fn has_pending_login(&self, telegram_user_id: &str) -> bool {
        self.pending_logins
            .lock()
            .await
            .contains_key(telegram_user_id)
    }

    pub(crate) async fn cancel_login(&self, telegram_user_id: &str) {
        if let Some(pending) = self.pending_logins.lock().await.remove(telegram_user_id) {
            pending.shutdown.cancel();
        }
    }

    pub(crate) async fn delete_account(
        &self,
        telegram_user_id: &str,
    ) -> Result<TelegramUserAccount, UserAccountError> {
        self.cancel_login(telegram_user_id).await;
        self.clear_lookup_cache(telegram_user_id).await;
        self.store
            .delete_user_account(telegram_user_id)
            .await
            .map_err(map_store)
    }

    pub(crate) async fn writable_groups(
        &self,
        telegram_user_id: &str,
    ) -> Result<Vec<TelegramUserGroup>, UserAccountError> {
        self.cached_writable_groups(telegram_user_id).await
    }

    pub(crate) async fn select_group(
        &self,
        telegram_user_id: &str,
        chat_id: &str,
        chat_type: &str,
    ) -> Result<TelegramUserAccount, UserAccountError> {
        let group = self
            .writable_groups(telegram_user_id)
            .await?
            .into_iter()
            .find(|group| group.chat_id == chat_id && group.chat_type == chat_type)
            .ok_or(UserAccountError::GroupNotWritable)?;
        self.store
            .set_selected_user_group(telegram_user_id, group)
            .await
            .map_err(map_store)
    }

    pub(crate) async fn send_text_to_selected_group(
        &self,
        telegram_user_id: &str,
        text: &str,
    ) -> Result<(), UserAccountError> {
        let account = self.account(telegram_user_id).await?;
        let selected_chat_id = account
            .selected_chat_id
            .as_deref()
            .ok_or(UserAccountError::GroupNotWritable)?;
        let selected_chat_type = account
            .selected_chat_type
            .as_deref()
            .ok_or(UserAccountError::GroupNotWritable)?;
        let (client, shutdown) = self.authorized_client(telegram_user_id).await?;
        let result =
            send_to_selected_group(&client, selected_chat_id, selected_chat_type, text).await;
        shutdown.cancel();
        result
    }

    pub(crate) async fn send_image_to_selected_group(
        &self,
        telegram_user_id: &str,
        text: &str,
        image: &CalculateOrderImage,
    ) -> Result<(), UserAccountError> {
        let account = self.account(telegram_user_id).await?;
        let selected_chat_id = account
            .selected_chat_id
            .as_deref()
            .ok_or(UserAccountError::GroupNotWritable)?;
        let selected_chat_type = account
            .selected_chat_type
            .as_deref()
            .ok_or(UserAccountError::GroupNotWritable)?;
        let (client, shutdown) = self.authorized_client(telegram_user_id).await?;
        let result = send_image_to_selected_group(
            &client,
            selected_chat_id,
            selected_chat_type,
            text,
            image,
        )
        .await;
        shutdown.cancel();
        result
    }

    async fn authorized_client(
        &self,
        telegram_user_id: &str,
    ) -> Result<(Client, ShutdownToken), UserAccountError> {
        let session = self
            .store
            .user_session(telegram_user_id)
            .await
            .map_err(map_store)?
            .ok_or(UserAccountError::NotAuthorized)?;
        let (api_id, api_hash) = api_credentials(&self.store).await?;
        let (client, shutdown) = connect_client(api_id, &api_hash, Some(&session)).await?;
        let disconnect = shutdown.clone().drop_guard();
        if !client.is_authorized().await.map_err(map_transport)? {
            shutdown.cancel();
            return Err(UserAccountError::NotAuthorized);
        }
        disconnect.disarm();
        Ok((client, shutdown))
    }

    async fn account(
        &self,
        telegram_user_id: &str,
    ) -> Result<TelegramUserAccount, UserAccountError> {
        self.store
            .user_by_telegram_id(telegram_user_id)
            .await
            .map_err(map_store)?
            .ok_or(UserAccountError::NotAuthorized)
    }

    async fn take_pending(&self, telegram_user_id: &str) -> Result<PendingLogin, UserAccountError> {
        self.pending_logins
            .lock()
            .await
            .remove(telegram_user_id)
            .ok_or(UserAccountError::LoginNotPending)
    }

    async fn put_pending(&self, telegram_user_id: &str, pending: PendingLogin) {
        self.pending_logins
            .lock()
            .await
            .insert(telegram_user_id.to_string(), pending);
    }

    async fn finish_login(
        &self,
        telegram_user_id: &str,
        phone_number: String,
        client: Client,
        shutdown: ShutdownToken,
    ) -> Result<TelegramUserAccount, UserAccountError> {
        let result = async {
            let me = client.get_me().await.map_err(map_transport)?;
            if me.id.to_string() != telegram_user_id {
                return Err(UserAccountError::AccountMismatch);
            }
            if me
                .phone
                .as_deref()
                .is_some_and(|phone| normalize_phone(phone) != phone_number)
            {
                return Err(UserAccountError::AccountMismatch);
            }
            let session = client
                .export_native_session_string()
                .await
                .map_err(map_transport)?;
            self.store
                .complete_user_profile_login(telegram_user_id, phone_number, session)
                .await
                .map_err(map_store)
        }
        .await;
        shutdown.cancel();
        result
    }
}

include!("connect_client.rs");
