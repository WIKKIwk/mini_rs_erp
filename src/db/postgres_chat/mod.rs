mod read;
mod realtime;
mod rows;
mod write;

use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::auth::models::Principal;
use crate::core::chat::{
    ChatConversation, ChatError, ChatMessagePage, ChatOutboxEvent, ChatPrincipal,
    ChatPrincipalInput, ChatPushDelivery, ChatRealtimeEvent, ChatSendResult, ChatStorePort,
    OrderFreezeChatEvent,
};

pub use realtime::start_realtime_listener;

#[derive(Clone)]
pub struct PostgresChatStore {
    pool: PgPool,
}

impl PostgresChatStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ChatStorePort for PostgresChatStore {
    async fn ensure_principal(
        &self,
        principal: ChatPrincipalInput,
    ) -> Result<ChatPrincipal, ChatError> {
        write::ensure_principal(&self.pool, principal).await
    }

    async fn create_or_get_dm(
        &self,
        actor: &ChatPrincipal,
        target: &ChatPrincipal,
    ) -> Result<ChatConversation, ChatError> {
        write::create_or_get_dm(&self.pool, actor, target).await
    }

    async fn conversations(
        &self,
        principal: &Principal,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ChatConversation>, ChatError> {
        read::conversations(&self.pool, principal, limit, offset).await
    }

    async fn messages(
        &self,
        principal: &Principal,
        conversation_id: &str,
        before_sequence: Option<i64>,
        after_sequence: Option<i64>,
        limit: usize,
    ) -> Result<ChatMessagePage, ChatError> {
        read::messages(
            &self.pool,
            principal,
            conversation_id,
            before_sequence,
            after_sequence,
            limit,
        )
        .await
    }

    async fn send_message(
        &self,
        principal: &Principal,
        conversation_id: &str,
        client_message_id: &str,
        body: &str,
    ) -> Result<ChatSendResult, ChatError> {
        write::send_message(
            &self.pool,
            principal,
            conversation_id,
            client_message_id,
            body,
            None,
        )
        .await
    }

    async fn send_media_message(
        &self,
        principal: &Principal,
        conversation_id: &str,
        client_message_id: &str,
        caption: &str,
        media_id: &str,
    ) -> Result<ChatSendResult, ChatError> {
        write::send_message(
            &self.pool,
            principal,
            conversation_id,
            client_message_id,
            caption,
            Some(media_id),
        )
        .await
    }

    async fn mark_read(
        &self,
        principal: &Principal,
        conversation_id: &str,
        sequence: i64,
        device_id: &str,
    ) -> Result<(), ChatError> {
        write::mark_read(&self.pool, principal, conversation_id, sequence, device_id).await
    }

    async fn mark_delivered(
        &self,
        principal: &Principal,
        conversation_id: &str,
        sequence: i64,
        device_id: &str,
    ) -> Result<(), ChatError> {
        write::mark_delivered(&self.pool, principal, conversation_id, sequence, device_id).await
    }

    async fn sync_events(
        &self,
        principal: &Principal,
        after_cursor: i64,
        limit: usize,
    ) -> Result<(Vec<ChatRealtimeEvent>, i64, bool), ChatError> {
        read::sync_events(&self.pool, principal, after_cursor, limit).await
    }

    async fn issue_socket_ticket(
        &self,
        principal: &Principal,
        ticket: &str,
        expires_at_unix: i64,
    ) -> Result<(), ChatError> {
        write::issue_socket_ticket(&self.pool, principal, ticket, expires_at_unix).await
    }

    async fn consume_socket_ticket(&self, ticket: &str) -> Result<Principal, ChatError> {
        write::consume_socket_ticket(&self.pool, ticket).await
    }

    async fn claim_push_deliveries(
        &self,
        limit: usize,
    ) -> Result<Vec<ChatPushDelivery>, ChatError> {
        write::claim_push_deliveries(&self.pool, limit).await
    }

    async fn mark_push_delivered(
        &self,
        event_id: &str,
        recipient_key: &str,
    ) -> Result<(), ChatError> {
        write::mark_push_delivered(&self.pool, event_id, recipient_key).await
    }

    async fn reschedule_push_delivery(
        &self,
        event_id: &str,
        recipient_key: &str,
        retry_after_seconds: i64,
        dead_letter: bool,
        error: &str,
    ) -> Result<(), ChatError> {
        write::reschedule_push_delivery(
            &self.pool,
            event_id,
            recipient_key,
            retry_after_seconds,
            dead_letter,
            error,
        )
        .await
    }

    async fn claim_outbox(&self, limit: usize) -> Result<Vec<ChatOutboxEvent>, ChatError> {
        write::claim_outbox(&self.pool, limit).await
    }

    async fn mark_outbox_published(&self, event_id: &str) -> Result<(), ChatError> {
        write::mark_outbox_published(&self.pool, event_id).await
    }

    async fn claim_order_freeze_chat_events(
        &self,
        limit: usize,
    ) -> Result<Vec<OrderFreezeChatEvent>, ChatError> {
        write::claim_order_freeze_chat_events(&self.pool, limit).await
    }

    async fn upsert_order_freeze_card(
        &self,
        principal: &Principal,
        conversation_id: &str,
        event: &OrderFreezeChatEvent,
    ) -> Result<ChatSendResult, ChatError> {
        write::upsert_order_freeze_card(&self.pool, principal, conversation_id, event).await
    }

    async fn mark_order_freeze_chat_event_delivered(
        &self,
        event_id: &str,
    ) -> Result<(), ChatError> {
        write::mark_order_freeze_chat_event_delivered(&self.pool, event_id).await
    }

    async fn reschedule_order_freeze_chat_event(
        &self,
        event_id: &str,
        error: &str,
    ) -> Result<(), ChatError> {
        write::reschedule_order_freeze_chat_event(&self.pool, event_id, error).await
    }
}
