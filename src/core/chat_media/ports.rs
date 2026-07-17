use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;

use super::{
    ChatMediaCreateResult, ChatMediaProcessedContent, ChatMediaProcessingWorkItem,
    ChatMediaReadyInput, ChatMediaStorageDownload, ChatMediaStorageObject,
    ChatMediaStorageUpload, ChatMediaStoredContent, ChatMediaUploadRecord,
    NewChatMediaUpload,
};
use crate::core::auth::models::Principal;

pub type ChatMediaByteStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, ChatMediaStorageError>> + Send + 'static>>;

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum ChatMediaError {
    #[error("chat media is unavailable")]
    Unavailable,
    #[error("chat media input is invalid")]
    InvalidInput,
    #[error("chat media is too large")]
    TooLarge,
    #[error("chat media upload was not found")]
    NotFound,
    #[error("chat media access is forbidden")]
    Forbidden,
    #[error("chat media state conflicts with this operation")]
    Conflict,
    #[error("chat media repository failed")]
    StoreFailed,
    #[error("chat media storage failed")]
    StorageFailed,
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum ChatMediaStorageError {
    #[error("chat media storage is unavailable")]
    Unavailable,
    #[error("chat media object key is invalid")]
    InvalidObjectKey,
    #[error("chat media object was not found")]
    ObjectNotFound,
    #[error("chat media object size does not match")]
    SizeMismatch,
    #[error("chat media requires a direct upload")]
    DirectUploadRequired,
    #[error("chat media storage operation failed")]
    OperationFailed,
}

#[async_trait]
pub trait ChatMediaRepository: Send + Sync {
    async fn initialize_upload(
        &self,
        principal: &Principal,
        upload: NewChatMediaUpload,
    ) -> Result<ChatMediaCreateResult, ChatMediaError>;

    async fn upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        require_can_post: bool,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError>;

    async fn mark_uploaded(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        storage: &ChatMediaStorageObject,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError>;

    async fn complete_upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        storage: &ChatMediaStorageObject,
        job_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError>;

    async fn cancel_upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError>;

    async fn claim_orphaned_uploads(
        &self,
        now_unix: i64,
        limit: usize,
    ) -> Result<Vec<ChatMediaUploadRecord>, ChatMediaError>;

    async fn mark_orphan_cleaned(&self, media_id: &str) -> Result<(), ChatMediaError>;

    async fn release_orphan_cleanup(&self, media_id: &str) -> Result<(), ChatMediaError>;

    async fn claim_processing_jobs(
        &self,
        limit: usize,
    ) -> Result<Vec<ChatMediaProcessingWorkItem>, ChatMediaError>;

    async fn mark_processing_ready(
        &self,
        job_id: &str,
        media_id: &str,
        ready: &ChatMediaReadyInput,
    ) -> Result<(), ChatMediaError>;

    async fn mark_processing_failed(
        &self,
        job_id: &str,
        media_id: &str,
        error_code: &str,
    ) -> Result<(), ChatMediaError>;

    async fn media_for_access(
        &self,
        principal: &Principal,
        media_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError>;
}

#[async_trait]
pub trait ChatMediaStorage: Send + Sync {
    async fn prepare_upload(
        &self,
        object_key: &str,
        content_type: &str,
        expected_size_bytes: i64,
    ) -> Result<ChatMediaStorageUpload, ChatMediaStorageError>;

    async fn put_object(
        &self,
        object_key: &str,
        content_type: &str,
        expected_size_bytes: i64,
        stream: ChatMediaByteStream,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError>;

    async fn object_metadata(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError>;

    async fn delete_object(&self, object_key: &str) -> Result<(), ChatMediaStorageError>;

    async fn read_object(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStoredContent, ChatMediaStorageError>;

    async fn put_private_object(
        &self,
        object_key: &str,
        content_type: &str,
        content: Bytes,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError>;

    async fn prepare_download(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStorageDownload, ChatMediaStorageError>;
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum ChatMediaProcessingError {
    #[error("media processing is unavailable")]
    Unavailable,
    #[error("uploaded media content is invalid")]
    InvalidContent,
    #[error("uploaded video exceeds the duration limit")]
    DurationTooLong,
    #[error("media processing failed")]
    ProcessingFailed,
}

#[async_trait]
pub trait ChatMediaProcessor: Send + Sync {
    async fn process(
        &self,
        media: &ChatMediaUploadRecord,
        source: Bytes,
    ) -> Result<ChatMediaProcessedContent, ChatMediaProcessingError>;
}
