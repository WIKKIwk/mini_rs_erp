mod read;
mod rows;
mod write;

use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::auth::models::Principal;
use crate::core::chat::{
    ChatConversation, ChatError, ChatMessagePage, ChatOutboxEvent, ChatPrincipal,
    ChatPrincipalInput, ChatSendResult, ChatStorePort,
};

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
        limit: usize,
    ) -> Result<ChatMessagePage, ChatError> {
        read::messages(
            &self.pool,
            principal,
            conversation_id,
            before_sequence,
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

    async fn claim_outbox(&self, limit: usize) -> Result<Vec<ChatOutboxEvent>, ChatError> {
        write::claim_outbox(&self.pool, limit).await
    }

    async fn mark_outbox_published(&self, event_id: &str) -> Result<(), ChatError> {
        write::mark_outbox_published(&self.pool, event_id).await
    }
}
