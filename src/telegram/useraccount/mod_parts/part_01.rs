
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
}

#[derive(Clone)]
pub(crate) struct TelegramUserAccountService {
    store: Arc<TelegramStore>,
    pending_logins: Arc<Mutex<BTreeMap<String, PendingLogin>>>,
}

impl TelegramUserAccountService {
    pub(crate) fn new(store: Arc<TelegramStore>) -> Self {
        Self {
            store,
            pending_logins: Arc::new(Mutex::new(BTreeMap::new())),
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
        let (api_id, api_hash) = api_credentials()?;
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

        match client
            .request_login_code(&phone_number)
            .await
            .map_err(map_transport)?
        {
            SendCodeOutcome::CodeRequired(login_token) => {
                let pending = PendingLogin {
                    client,
                    shutdown,
                    login_token,
                    password_token: None,
                    phone_number,
                };
                let mut logins = self.pending_logins.lock().await;
                if let Some(previous) = logins.insert(telegram_user_id.to_string(), pending) {
                    previous.shutdown.cancel();
                }
                Ok(LoginOutcome::CodeSent)
            }
            SendCodeOutcome::AlreadyAuthorized(_) => self
                .finish_login(telegram_user_id, phone_number, client, shutdown)
                .await
                .map(|_| LoginOutcome::Authorized),
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

    pub(crate) async fn writable_groups(
        &self,
        telegram_user_id: &str,
    ) -> Result<Vec<TelegramUserGroup>, UserAccountError> {
        let (client, shutdown) = self.authorized_client(telegram_user_id).await?;
        let result = list_writable_groups(&client).await;
        shutdown.cancel();
        result
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
        let (api_id, api_hash) = api_credentials()?;
        let (client, shutdown) = connect_client(api_id, &api_hash, Some(&session)).await?;
        if !client.is_authorized().await.map_err(map_transport)? {
            shutdown.cancel();
            return Err(UserAccountError::NotAuthorized);
        }
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

async fn connect_client(
    api_id: i32,
    api_hash: &str,
    session: Option<&str>,
) -> Result<(Client, ShutdownToken), UserAccountError> {
    ferogram::Client::builder()
        .api_id(api_id)
        .api_hash(api_hash)
        .session_string(session.unwrap_or_default())
        .transport(TransportKind::Abridged)
        .probe_transport(true)
        .resilient_connect(true)
        .connect()
        .await
        .map_err(|error| UserAccountError::Transport(error.to_string()))
}

fn api_credentials() -> Result<(i32, String), UserAccountError> {
    let api_id = env::var("TELEGRAM_API_ID")
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .filter(|value| *value > 0)
        .ok_or(UserAccountError::NotConfigured)?;
    let api_hash = env::var("TELEGRAM_API_HASH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or(UserAccountError::NotConfigured)?;
    Ok((api_id, api_hash))
}

async fn list_writable_groups(client: &Client) -> Result<Vec<TelegramUserGroup>, UserAccountError> {
    let mut groups = Vec::new();
    for folder_id in [0, 1] {
        let mut iter = client.iter_dialogs().folder_id(Some(folder_id));
        while let Some(dialog) = iter.next(client).await.map_err(map_transport)? {
            if let Some(group) = writable_group_from_dialog(&dialog) {
                groups.push(group);
            }
        }
    }
    groups.sort_by(|left, right| {
        left.chat_type
            .cmp(&right.chat_type)
            .then_with(|| left.chat_id.cmp(&right.chat_id))
    });
    groups
        .dedup_by(|left, right| left.chat_type == right.chat_type && left.chat_id == right.chat_id);
    groups.sort_by(|left, right| {
        left.title
            .to_lowercase()
            .cmp(&right.title.to_lowercase())
            .then_with(|| left.chat_id.cmp(&right.chat_id))
    });
    Ok(groups)
}

async fn send_to_selected_group(
    client: &Client,
    selected_chat_id: &str,
    selected_chat_type: &str,
    text: &str,
) -> Result<(), UserAccountError> {
    let peer = selected_group_peer(client, selected_chat_id, selected_chat_type).await?;
    client
        .send_message(peer, InputMessage::text(text))
        .await
        .map_err(map_transport)?;
    Ok(())
}
