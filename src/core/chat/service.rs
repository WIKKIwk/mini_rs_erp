use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::sleep;

use super::{
    ChatConversation, ChatConversationPage, ChatError, ChatHub, ChatMessagePage, ChatPrincipal,
    ChatPrincipalInput, ChatSendResult, ChatStorePort,
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
                match service.store.claim_outbox(100).await {
                    Ok(events) if events.is_empty() => sleep(Duration::from_millis(350)).await,
                    Ok(events) => {
                        for event in events {
                            service
                                .hub
                                .publish(event.payload.clone(), &event.recipient_keys)
                                .await;
                            let mut delivered = true;
                            for recipient in &event.recipient_keys {
                                let mut data = HashMap::new();
                                data.insert(
                                    "event_type".to_string(),
                                    "chat.message.created".to_string(),
                                );
                                data.insert(
                                    "conversation_id".to_string(),
                                    event.payload.conversation_id.clone(),
                                );
                                data.insert(
                                    "message_id".to_string(),
                                    event.payload.message.message_id.clone(),
                                );
                                if let Some((role, ref_)) = recipient.split_once(':') {
                                    data.insert("target_role".to_string(), role.to_string());
                                    data.insert("target_ref".to_string(), ref_.to_string());
                                }
                                if push
                                    .send_to_key(
                                        recipient,
                                        &event.payload.message.sender_display_name,
                                        &message_preview(&event.payload.message.body),
                                        data,
                                    )
                                    .await
                                    .is_err()
                                {
                                    delivered = false;
                                }
                            }
                            if delivered
                                && let Err(error) =
                                    service.store.mark_outbox_published(&event.event_id).await
                            {
                                tracing::warn!(%error, "chat outbox publish marker failed");
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "chat outbox worker failed");
                        sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }
}

fn message_preview(body: &str) -> String {
    let mut chars = body.chars();
    let preview = chars.by_ref().take(120).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

struct UnavailableChatStore;

#[async_trait::async_trait]
impl ChatStorePort for UnavailableChatStore {
    async fn ensure_principal(
        &self,
        _principal: ChatPrincipalInput,
    ) -> Result<ChatPrincipal, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn create_or_get_dm(
        &self,
        _actor: &ChatPrincipal,
        _target: &ChatPrincipal,
    ) -> Result<ChatConversation, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn conversations(
        &self,
        _principal: &Principal,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<ChatConversation>, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn messages(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _before_sequence: Option<i64>,
        _limit: usize,
    ) -> Result<ChatMessagePage, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn send_message(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _client_message_id: &str,
        _body: &str,
    ) -> Result<ChatSendResult, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn mark_read(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _sequence: i64,
        _device_id: &str,
    ) -> Result<(), ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn claim_outbox(&self, _limit: usize) -> Result<Vec<super::ChatOutboxEvent>, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn mark_outbox_published(&self, _event_id: &str) -> Result<(), ChatError> {
        Err(ChatError::Unavailable)
    }
}
