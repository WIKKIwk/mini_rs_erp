use async_trait::async_trait;

use super::{
    ChatMediaByteStream, ChatMediaCreateResult, ChatMediaError, ChatMediaProcessingWorkItem,
    ChatMediaReadyInput, ChatMediaRepository, ChatMediaStorage, ChatMediaStorageDownload,
    ChatMediaStorageError, ChatMediaStorageObject, ChatMediaStorageUpload,
    ChatMediaStoredContent, ChatMediaUploadRecord, ChatMediaUploadedChunk,
    NewChatMediaUpload, NewChatMediaUploadedChunk,
};
use crate::core::auth::models::Principal;

pub(super) struct UnavailableChatMediaRepository;

#[async_trait]
impl ChatMediaRepository for UnavailableChatMediaRepository {
    async fn initialize_upload(
        &self,
        _principal: &Principal,
        _upload: NewChatMediaUpload,
    ) -> Result<ChatMediaCreateResult, ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn upload(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _upload_id: &str,
        _require_can_post: bool,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn set_multipart_upload_id(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _upload_id: &str,
        _storage_upload_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn uploaded_chunks(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _upload_id: &str,
        _require_can_post: bool,
    ) -> Result<Vec<ChatMediaUploadedChunk>, ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn record_uploaded_chunk(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _upload_id: &str,
        _chunk: NewChatMediaUploadedChunk,
    ) -> Result<ChatMediaUploadedChunk, ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn mark_uploaded(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _upload_id: &str,
        _storage: &ChatMediaStorageObject,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn complete_upload(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _upload_id: &str,
        _storage: &ChatMediaStorageObject,
        _job_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn cancel_upload(
        &self,
        _principal: &Principal,
        _conversation_id: &str,
        _upload_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn claim_orphaned_uploads(
        &self,
        _now_unix: i64,
        _limit: usize,
    ) -> Result<Vec<ChatMediaUploadRecord>, ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn mark_orphan_cleaned(&self, _media_id: &str) -> Result<(), ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn release_orphan_cleanup(&self, _media_id: &str) -> Result<(), ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn claim_processing_jobs(
        &self,
        _limit: usize,
    ) -> Result<Vec<ChatMediaProcessingWorkItem>, ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn mark_processing_ready(
        &self,
        _job_id: &str,
        _media_id: &str,
        _ready: &ChatMediaReadyInput,
    ) -> Result<(), ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn mark_processing_failed(
        &self,
        _job_id: &str,
        _media_id: &str,
        _error_code: &str,
    ) -> Result<(), ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }

    async fn media_for_access(
        &self,
        _principal: &Principal,
        _media_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        Err(ChatMediaError::Unavailable)
    }
}

pub(super) struct UnavailableChatMediaStorage;

#[async_trait]
impl ChatMediaStorage for UnavailableChatMediaStorage {
    async fn prepare_upload(
        &self,
        _object_key: &str,
        _content_type: &str,
        _expected_size_bytes: i64,
    ) -> Result<ChatMediaStorageUpload, ChatMediaStorageError> {
        Err(ChatMediaStorageError::Unavailable)
    }

    async fn put_object(
        &self,
        _object_key: &str,
        _content_type: &str,
        _expected_size_bytes: i64,
        _stream: ChatMediaByteStream,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        Err(ChatMediaStorageError::Unavailable)
    }

    async fn object_metadata(
        &self,
        _object_key: &str,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        Err(ChatMediaStorageError::Unavailable)
    }

    async fn delete_object(&self, _object_key: &str) -> Result<(), ChatMediaStorageError> {
        Err(ChatMediaStorageError::Unavailable)
    }

    async fn read_object(
        &self,
        _object_key: &str,
    ) -> Result<ChatMediaStoredContent, ChatMediaStorageError> {
        Err(ChatMediaStorageError::Unavailable)
    }

    async fn put_private_object(
        &self,
        _object_key: &str,
        _content_type: &str,
        _content: bytes::Bytes,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        Err(ChatMediaStorageError::Unavailable)
    }

    async fn prepare_download(
        &self,
        _object_key: &str,
    ) -> Result<ChatMediaStorageDownload, ChatMediaStorageError> {
        Err(ChatMediaStorageError::Unavailable)
    }
}
