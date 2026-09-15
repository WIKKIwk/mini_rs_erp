impl TelegramService {
    pub fn new(path: PathBuf) -> Self {
        let store = Arc::new(TelegramStore::new(path));
        Self {
            useraccount: TelegramUserAccountService::new(store.clone()),
            qr_logins: super::useraccount::qr::QrLoginService::new(store.clone()),
            store,
            http: reqwest::Client::new(),
            worker_started: Arc::new(AtomicBool::new(false)),
            order_catalog: None,
            pending_orders: None,
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

    pub fn with_pending_orders(
        mut self,
        store: Option<Arc<dyn crate::core::pending_orders::PendingOrderStore>>,
    ) -> Self {
        self.pending_orders = store;
        self
    }

    pub(crate) async fn persist_pending_order(
        &self,
        account: &TelegramUserAccount,
        draft: &TelegramOrderDraft,
        image: CalculateOrderImage,
    ) -> Result<CalculateOrderImage, TelegramError> {
        let store = self
            .pending_orders
            .as_ref()
            .ok_or(TelegramError::OrderCatalogNotConfigured)?;
        let order = crate::core::pending_orders::PendingOrder {
            id: format!("zakaz-{}", draft.order_number),
            telegram_user_id: account.telegram_user_id.clone(),
            manager_name: account.display_name.clone(),
            created_at: OffsetDateTime::now_utc().unix_timestamp(),
            completion: None,
            template: CalculateOrderTemplate {
                id: format!("telegram-template-{:032x}", rand::random::<u128>()),
                code: format!("TG-{:032x}", rand::random::<u128>()),
                name: draft.product_name.clone(),
                order_number: draft.order_number.clone(),
                customer_ref: draft.customer_ref.clone(),
                customer: draft.customer_name.clone(),
                item_code: draft.product_code.clone(),
                product: draft.product_name.clone(),
                status: draft.status.clone(),
                kg: draft.tiraj_kg.unwrap_or_default(),
                frame_product_size_mm: draft.frame_product_size_mm.unwrap_or_default(),
                frame_count: draft.frame_count.unwrap_or_default(),
                edge_allowance_mm: crate::core::formula::DEFAULT_EDGE_ALLOWANCE_MM,
                waste_percent: 5.0,
                layers: draft
                    .layers
                    .iter()
                    .map(|l| {
                        crate::core::formula::LayerInput::with_material_id(
                            &l.material_id,
                            &l.material,
                            &l.micron,
                        )
                    })
                    .collect(),
                image_id: image.image_id.clone(),
                image_name: image.image_name.clone(),
                image_mime: image.image_mime.clone(),
                image_size_bytes: image.image_size_bytes,
                image_url: format!(
                    "/v1/mobile/admin/pending-orders/image?id=zakaz-{}",
                    draft.order_number
                ),
                ..CalculateOrderTemplate::default()
            },
        };
        let saved = store
            .create(order, image)
            .await
            .map_err(|e| TelegramError::OrderCatalog(e.to_string()))?;
        if saved.completion.is_some() {
            return Err(TelegramError::OrderCatalog(
                "Bu order mobile’da allaqachon tugallangan. /cancel bosing.".into(),
            ));
        }
        // Delivery retries use the durable original image, not a replacement attachment.
        store
            .image(&saved.id)
            .await
            .map_err(|e| TelegramError::OrderCatalog(e.to_string()))
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

    // Only Bot API updates may recognize an existing user without an invite.
    // The public /telegram/start endpoint must still require the invite token.
    pub(crate) async fn register_bot_start(
        &self,
        input: TelegramStartRequest,
    ) -> Result<TelegramUserAccount, TelegramError> {
        if !input.invite_token.trim().is_empty() {
            return self.register_start(input).await;
        }
        self.store
            .recognize_start(input.telegram_user_id.trim(), input.telegram_chat_id.trim())
            .await
            .map_err(map_store)?
            .ok_or(TelegramError::InviteTokenRequired)
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

    pub async fn delete_user_account(
        &self,
        telegram_user_id: &str,
    ) -> Result<TelegramUserAccount, TelegramError> {
        self.useraccount
            .delete_account(telegram_user_id.trim())
            .await
            .map_err(map_user_account)
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

    pub(crate) async fn resend_user_profile_code(
        &self,
        telegram_user_id: &str,
    ) -> Result<ResendOutcome, TelegramError> {
        self.useraccount
            .resend_code(telegram_user_id)
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

    pub(crate) async fn send_order_image_to_user_profile(
        &self,
        telegram_user_id: &str,
        caption: &str,
        image: &CalculateOrderImage,
    ) -> Result<(), TelegramError> {
        self.useraccount
            .send_image_to_selected_group(telegram_user_id, caption, image)
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

    pub(crate) async fn deliver_order(
        &self,
        telegram_user_id: &str,
        caption: &str,
        image: Option<CalculateOrderImage>,
    ) -> Result<usize, TelegramError> {
        let Some(account) = self.user_by_telegram_id(telegram_user_id).await? else {
            return Err(TelegramError::UserAccountNotAuthorized);
        };
        if account.delivery_mode == TelegramDeliveryMode::UserProfile {
            if let Some(image) = image.as_ref() {
                self.send_order_image_to_user_profile(telegram_user_id, caption, image)
                    .await?;
            } else {
                self.send_order_to_user_profile(telegram_user_id, caption)
                    .await?;
            }
            return Ok(1);
        }
        let chats = self.store.chats().await.map_err(map_store)?;
        let notification = super::bot::TelegramOrderNotification {
            caption: caption.to_string(),
            image,
        };
        let mut delivered = 0;
        let mut last_error = None;
        for chat in chats {
            match super::bot::send_order_to_chat(self, &chat, &notification).await {
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
        if delivered == 0
            && let Some(error) = last_error
        {
            return Err(error);
        }
        Ok(delivered)
    }
}
