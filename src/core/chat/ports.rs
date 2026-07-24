use async_trait::async_trait;

use super::{
    ChatConversation, ChatMessagePage, ChatOutboxEvent, ChatPrincipal, ChatPrincipalInput,
    ChatPushDelivery, ChatRealtimeEvent, ChatSendResult, OrderFreezeChatEvent,
};
use crate::core::auth::models::Principal;

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum ChatError {
    #[error("chat is unavailable")]
    Unavailable,
    #[error("chat input is invalid")]
    InvalidInput,
    #[error("chat principal or conversation was not found")]
    NotFound,
    #[error("chat access is forbidden")]
    Forbidden,
    #[error("chat state conflicts with this operation")]
    Conflict,
    #[error("chat store failed")]
    StoreFailed,
}

#[async_trait]
pub trait ChatStorePort: Send + Sync {
    async fn ensure_principal(
        &self,
        principal: ChatPrincipalInput,
    ) -> Result<ChatPrincipal, ChatError>;

    async fn create_or_get_dm(
        &self,
        actor: &ChatPrincipal,
        target: &ChatPrincipal,
    ) -> Result<ChatConversation, ChatError>;

    async fn conversations(
        &self,
        principal: &Principal,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ChatConversation>, ChatError>;

    async fn messages(
        &self,
        principal: &Principal,
        conversation_id: &str,
        before_sequence: Option<i64>,
        after_sequence: Option<i64>,
        limit: usize,
    ) -> Result<ChatMessagePage, ChatError>;

    async fn send_message(
        &self,
        principal: &Principal,
        conversation_id: &str,
        client_message_id: &str,
        body: &str,
    ) -> Result<ChatSendResult, ChatError>;

    async fn send_media_message(
        &self,
        principal: &Principal,
        conversation_id: &str,
        client_message_id: &str,
        caption: &str,
        media_id: &str,
    ) -> Result<ChatSendResult, ChatError>;

    async fn mark_read(
        &self,
        principal: &Principal,
        conversation_id: &str,
        sequence: i64,
        device_id: &str,
    ) -> Result<(), ChatError>;

    async fn mark_delivered(
        &self,
        principal: &Principal,
        conversation_id: &str,
        sequence: i64,
        device_id: &str,
    ) -> Result<(), ChatError>;

    async fn sync_events(
        &self,
        principal: &Principal,
        after_cursor: i64,
        limit: usize,
    ) -> Result<(Vec<ChatRealtimeEvent>, i64, bool), ChatError>;

    async fn issue_socket_ticket(
        &self,
        principal: &Principal,
        ticket: &str,
        expires_at_unix: i64,
    ) -> Result<(), ChatError>;

    async fn consume_socket_ticket(&self, ticket: &str) -> Result<Principal, ChatError>;

    async fn claim_push_deliveries(&self, limit: usize)
    -> Result<Vec<ChatPushDelivery>, ChatError>;

    async fn mark_push_delivered(
        &self,
        event_id: &str,
        recipient_key: &str,
    ) -> Result<(), ChatError>;

    async fn reschedule_push_delivery(
        &self,
        event_id: &str,
        recipient_key: &str,
        retry_after_seconds: i64,
        dead_letter: bool,
        error: &str,
    ) -> Result<(), ChatError>;

    async fn claim_outbox(&self, limit: usize) -> Result<Vec<ChatOutboxEvent>, ChatError>;

    async fn mark_outbox_published(&self, event_id: &str) -> Result<(), ChatError>;

    async fn claim_order_freeze_chat_events(
        &self,
        _limit: usize,
    ) -> Result<Vec<OrderFreezeChatEvent>, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn upsert_order_freeze_card(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _event: &OrderFreezeChatEvent,
    ) -> Result<ChatSendResult, ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn mark_order_freeze_chat_event_delivered(
        &self,
        _event_id: &str,
    ) -> Result<(), ChatError> {
        Err(ChatError::Unavailable)
    }

    async fn reschedule_order_freeze_chat_event(
        &self,
        _event_id: &str,
        _error: &str,
    ) -> Result<(), ChatError> {
        Err(ChatError::Unavailable)
    }
}
