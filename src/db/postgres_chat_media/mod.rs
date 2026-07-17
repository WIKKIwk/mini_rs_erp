mod repository;
mod rows;

#[cfg(test)]
mod tests;

use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::auth::models::Principal;
use crate::core::chat_media::{
    ChatMediaCreateResult, ChatMediaError, ChatMediaRepository, ChatMediaStorageObject,
    ChatMediaUploadRecord, NewChatMediaUpload,
};

#[derive(Clone)]
pub struct PostgresChatMediaRepository {
    pool: PgPool,
}

impl PostgresChatMediaRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ChatMediaRepository for PostgresChatMediaRepository {
    async fn initialize_upload(
        &self,
        principal: &Principal,
        upload: NewChatMediaUpload,
    ) -> Result<ChatMediaCreateResult, ChatMediaError> {
        repository::initialize_upload(&self.pool, principal, upload).await
    }

    async fn upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        require_can_post: bool,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        repository::upload(
            &self.pool,
            principal,
            conversation_id,
            upload_id,
            require_can_post,
        )
        .await
    }

    async fn mark_uploaded(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        storage: &ChatMediaStorageObject,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        repository::mark_uploaded(&self.pool, principal, conversation_id, upload_id, storage).await
    }

    async fn complete_upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        storage: &ChatMediaStorageObject,
        job_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        repository::complete_upload(
            &self.pool,
            principal,
            conversation_id,
            upload_id,
            storage,
            job_id,
        )
        .await
    }

    async fn cancel_upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        repository::cancel_upload(&self.pool, principal, conversation_id, upload_id).await
    }

    async fn claim_orphaned_uploads(
        &self,
        now_unix: i64,
        limit: usize,
    ) -> Result<Vec<ChatMediaUploadRecord>, ChatMediaError> {
        repository::claim_orphaned_uploads(&self.pool, now_unix, limit).await
    }

    async fn mark_orphan_cleaned(&self, media_id: &str) -> Result<(), ChatMediaError> {
        repository::mark_orphan_cleaned(&self.pool, media_id).await
    }

    async fn release_orphan_cleanup(&self, media_id: &str) -> Result<(), ChatMediaError> {
        repository::release_orphan_cleanup(&self.pool, media_id).await
    }
}
