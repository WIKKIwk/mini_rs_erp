use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use tokio::task::JoinSet;
use tokio::time::sleep;

use super::{
    ChatConversation, ChatConversationPage, ChatError, ChatHub, ChatMessagePage, ChatPrincipal,
    ChatPrincipalInput, ChatSendResult, ChatStorePort, ChatSyncPage,
};
use crate::core::auth::models::Principal;
use crate::core::push::service::PushService;

#[derive(Clone)]
pub struct ChatService {
    store: Arc<dyn ChatStorePort>,
    hub: ChatHub,
    delivery_enabled: bool,
}

impl ChatService {
    pub fn new(store: Arc<dyn ChatStorePort>) -> Self {
        Self {
            store,
            hub: ChatHub::new(),
            delivery_enabled: true,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            store: Arc::new(UnavailableChatStore),
            hub: ChatHub::new(),
            delivery_enabled: false,
        }
    }

    pub fn hub(&self) -> &ChatHub {
        &self.hub
    }

    pub async fn ensure_principal(
        &self,
        principal: ChatPrincipalInput,
    ) -> Result<ChatPrincipal, ChatError> {
        if principal.ref_.trim().is_empty() || principal.display_name.trim().is_empty() {
            return Err(ChatError::InvalidInput);
        }
        self.store.ensure_principal(principal).await
    }

    pub async fn create_or_get_dm(
        &self,
        actor: ChatPrincipalInput,
        target: ChatPrincipalInput,
    ) -> Result<ChatConversation, ChatError> {
        if actor.role == target.role && actor.ref_.trim() == target.ref_.trim() {
            return Err(ChatError::InvalidInput);
        }
        let actor = self.ensure_principal(actor).await?;
        let target = self.ensure_principal(target).await?;
        self.store.create_or_get_dm(&actor, &target).await
    }

    pub async fn conversations(
        &self,
        principal: &Principal,
        limit: usize,
        offset: usize,
    ) -> Result<ChatConversationPage, ChatError> {
        let limit = limit.clamp(1, 100);
        let mut items = self
            .store
            .conversations(principal, limit.saturating_add(1), offset)
            .await?;
        let has_more = items.len() > limit;
        items.truncate(limit);
        Ok(ChatConversationPage { items, has_more })
    }

    pub async fn messages(
        &self,
        principal: &Principal,
        conversation_id: &str,
        before_sequence: Option<i64>,
        after_sequence: Option<i64>,
        limit: usize,
    ) -> Result<ChatMessagePage, ChatError> {
        if conversation_id.trim().is_empty() {
            return Err(ChatError::InvalidInput);
        }
        self.store
            .messages(
                principal,
                conversation_id.trim(),
                before_sequence.filter(|value| *value > 0),
                after_sequence.filter(|value| *value >= 0),
                limit.clamp(1, 100),
            )
            .await
    }

    pub async fn send_message(
        &self,
        principal: &Principal,
        conversation_id: &str,
        client_message_id: &str,
        body: &str,
    ) -> Result<ChatSendResult, ChatError> {
        let body = body.trim();
        if conversation_id.trim().is_empty()
            || client_message_id.trim().is_empty()
            || body.is_empty()
            || body.chars().count() > 4000
        {
            return Err(ChatError::InvalidInput);
        }
        self.store
            .send_message(
                principal,
                conversation_id.trim(),
                client_message_id.trim(),
                body,
            )
            .await
    }

    pub async fn send_media_message(
        &self,
        principal: &Principal,
        conversation_id: &str,
        client_message_id: &str,
        caption: &str,
        media_id: &str,
    ) -> Result<ChatSendResult, ChatError> {
        let caption = caption.trim();
        if conversation_id.trim().is_empty()
            || client_message_id.trim().is_empty()
            || media_id.trim().is_empty()
            || caption.chars().count() > 4000
        {
            return Err(ChatError::InvalidInput);
        }
        self.store
            .send_media_message(
                principal,
                conversation_id.trim(),
                client_message_id.trim(),
                caption,
                media_id.trim(),
            )
            .await
    }

    pub async fn mark_read(
        &self,
        principal: &Principal,
        conversation_id: &str,
        sequence: i64,
        device_id: &str,
    ) -> Result<(), ChatError> {
        if conversation_id.trim().is_empty() || sequence < 0 || device_id.trim().is_empty() {
            return Err(ChatError::InvalidInput);
        }
        self.store
            .mark_read(
                principal,
                conversation_id.trim(),
                sequence,
                device_id.trim(),
            )
            .await
    }

    pub async fn mark_delivered(
        &self,
        principal: &Principal,
        conversation_id: &str,
        sequence: i64,
        device_id: &str,
    ) -> Result<(), ChatError> {
        if conversation_id.trim().is_empty() || sequence < 0 || device_id.trim().is_empty() {
            return Err(ChatError::InvalidInput);
        }
        self.store
            .mark_delivered(
                principal,
                conversation_id.trim(),
                sequence,
                device_id.trim(),
            )
            .await
    }

    pub async fn sync(
        &self,
        principal: &Principal,
        after_cursor: i64,
        limit: usize,
    ) -> Result<ChatSyncPage, ChatError> {
        let (events, next_cursor, has_more) = self
            .store
            .sync_events(principal, after_cursor.max(0), limit.clamp(1, 500))
            .await?;
        Ok(ChatSyncPage {
            events,
            next_cursor,
            has_more,
        })
    }

    pub async fn issue_socket_ticket(
        &self,
        principal: Principal,
    ) -> Result<(String, i64), ChatError> {
        let mut bytes = [0_u8; 24];
        rand::fill(&mut bytes);
        let ticket = URL_SAFE_NO_PAD.encode(bytes);
        let expires_at_unix = time::OffsetDateTime::now_utc().unix_timestamp() + 30;
        match self
            .store
            .issue_socket_ticket(&principal, &ticket, expires_at_unix)
            .await
        {
            Ok(()) => Ok((ticket, expires_at_unix)),
            Err(ChatError::Unavailable) => {
                let ticket = self.hub.issue_ticket(principal).await;
                Ok((ticket, expires_at_unix))
            }
            Err(error) => Err(error),
        }
    }

    pub async fn consume_socket_ticket(&self, ticket: &str) -> Result<Principal, ChatError> {
        match self.store.consume_socket_ticket(ticket.trim()).await {
            Ok(principal) => Ok(principal),
            Err(ChatError::Unavailable) => self
                .hub
                .consume_ticket(ticket)
                .await
                .ok_or(ChatError::NotFound),
            Err(error) => Err(error),
        }
    }

    pub fn start_delivery_worker(&self, push: PushService) {
        if !self.delivery_enabled {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let service = self.clone();
        handle.spawn(async move {
            loop {
                match service.store.claim_push_deliveries(32).await {
                    Ok(deliveries) if deliveries.is_empty() => {
                        sleep(Duration::from_millis(350)).await
                    }
                    Ok(deliveries) => {
                        let mut tasks = JoinSet::new();
                        for delivery in deliveries {
                            let service = service.clone();
                            let push = push.clone();
                            tasks.spawn(async move {
                                service.deliver_push(push, delivery).await;
                            });
                        }
                        while tasks.join_next().await.is_some() {}
                    }
                    Err(error) => {
                        tracing::warn!(%error, "chat outbox worker failed");
                        sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }

    async fn deliver_push(&self, push: PushService, delivery: super::ChatPushDelivery) {
        let message = &delivery.payload.message;
        let mut data = HashMap::new();
        data.insert("event_type".to_string(), "chat.message.created".to_string());
        data.insert(
            "conversation_id".to_string(),
            delivery.payload.conversation_id.clone(),
        );
        data.insert("message_id".to_string(), message.message_id.clone());
        data.insert("message_type".to_string(), message.message_type.clone());
        data.insert(
            "event_cursor".to_string(),
            delivery.payload.cursor.to_string(),
        );
        if let Some((role, ref_)) = delivery.recipient_key.split_once(':') {
            data.insert("target_role".to_string(), role.to_string());
            data.insert("target_ref".to_string(), ref_.to_string());
        }
        match push
            .send_to_key(
                &delivery.recipient_key,
                &message.sender_display_name,
                &message_preview(message),
                data,
            )
            .await
        {
            Ok(()) => {
                if let Err(error) = self
                    .store
                    .mark_push_delivered(&delivery.event_id, &delivery.recipient_key)
                    .await
                {
                    tracing::warn!(%error, event_id = %delivery.event_id, "chat push marker failed");
                }
            }
            Err(error) => {
                const MAX_ATTEMPTS: i32 = 8;
                let dead_letter = delivery.attempts >= MAX_ATTEMPTS;
                let retry_after = push_retry_delay_seconds(delivery.attempts);
                if let Err(store_error) = self
                    .store
                    .reschedule_push_delivery(
                        &delivery.event_id,
                        &delivery.recipient_key,
                        retry_after,
                        dead_letter,
                        &error.to_string(),
                    )
                    .await
                {
                    tracing::warn!(%store_error, event_id = %delivery.event_id, "chat push retry marker failed");
                }
            }
        }
    }
}

fn push_retry_delay_seconds(attempts: i32) -> i64 {
    let exponent = attempts.clamp(1, 10) as u32;
    (1_i64 << exponent).clamp(2, 900)
}

fn message_preview(message: &super::ChatMessage) -> String {
    message_preview_text(&message.message_type, &message.body)
}

fn message_preview_text(message_type: &str, body: &str) -> String {
    let fallback = match message_type {
        "image" => "Rasm",
        "video" => "Video",
        "audio" => "Ovozli xabar",
        _ => "Xabar",
    };
    let body = body.trim();
    let mut chars = if body.is_empty() { fallback } else { body }.chars();
    let preview = chars.by_ref().take(120).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

include!("service_inline_tests.rs");
