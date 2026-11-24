impl TelegramService {
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
        self.clear_order_attachment(telegram_user_id).await;
        self.store
            .clear_order_draft(telegram_user_id)
            .await
            .map_err(map_store)
    }

    pub(crate) async fn save_order_attachment(
        &self,
        telegram_user_id: &str,
        attachment: TelegramOrderAttachment,
    ) {
        self.order_attachments
            .lock()
            .await
            .insert(telegram_user_id.to_string(), attachment);
    }

    pub(crate) async fn order_attachment(
        &self,
        telegram_user_id: &str,
    ) -> Option<TelegramOrderAttachment> {
        self.order_attachments
            .lock()
            .await
            .get(telegram_user_id)
            .cloned()
    }

    pub(crate) async fn clear_order_attachment(&self, telegram_user_id: &str) {
        self.order_attachments
            .lock()
            .await
            .remove(telegram_user_id);
    }

    pub(crate) async fn pending_order_image(
        &self,
        order_id: &str,
    ) -> Result<Option<CalculateOrderImage>, TelegramError> {
        let Some(store) = self.pending_orders.as_ref() else {
            return Ok(None);
        };
        store
            .image(order_id)
            .await
            .map(Some)
            .map_err(|error| TelegramError::OrderCatalog(error.to_string()))
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

    pub(crate) async fn polling_state(&self) -> (String, i64) {
        self.store.polling_state().await
    }

    pub(crate) async fn set_update_offset(
        &self,
        token: &str,
        offset: i64,
    ) -> Result<(), TelegramError> {
        self.store
            .set_update_offset(token, offset)
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
