use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use super::service_support::{configured_video_chunk_size, map_storage_error, new_id};
use super::{
    ChatMediaAccess, ChatMediaAccessVariant, ChatMediaByteStream, ChatMediaCreateResult,
    ChatMediaError, ChatMediaInitialization, ChatMediaInitializeInput, ChatMediaKind,
    ChatMediaProcessor, ChatMediaRangeRequest, ChatMediaRepository, ChatMediaStatus,
    ChatMediaStorage, ChatMediaStorageDownload, ChatMediaStorageError, ChatMediaStorageObject,
    ChatMediaStreamAccess, ChatMediaUploadMode, ChatMediaUploadRecord, ChatMediaUploadView,
    NewChatMediaUpload, SystemChatMediaProcessor,
};
use crate::core::auth::models::Principal;

pub const MAX_CHAT_IMAGE_SIZE_BYTES: i64 = 15 * 1024 * 1024;
pub const MAX_CHAT_VIDEO_SIZE_BYTES: i64 = 2 * 1024 * 1024 * 1024;
pub const MAX_CHAT_PROCESSED_VIDEO_SIZE_BYTES: i64 = 1024 * 1024 * 1024;
pub const MAX_CHAT_VIDEO_DURATION_MS: i64 = 600_000;
pub const MAX_CHAT_AUDIO_SIZE_BYTES: i64 = 64 * 1024 * 1024;
pub const MAX_CHAT_PROCESSED_AUDIO_SIZE_BYTES: i64 = 64 * 1024 * 1024;
pub const MAX_CHAT_AUDIO_DURATION_MS: i64 = 600_000;
pub const DEFAULT_CHAT_VIDEO_CHUNK_SIZE_BYTES: i64 = 8 * 1024 * 1024;
pub const MAX_CHAT_MEDIA_CHUNK_SIZE_BYTES: i64 = 64 * 1024 * 1024;
pub(super) const MIN_CHAT_MEDIA_CHUNK_SIZE_BYTES: i64 = 5 * 1024 * 1024;

const UPLOAD_RETENTION_SECONDS: i64 = 24 * 60 * 60;
const PLAYBACK_TICKET_TTL_SECONDS: i64 = 60 * 60;

#[derive(Clone)]
pub struct ChatMediaService {
    pub(super) repository: Arc<dyn ChatMediaRepository>,
    pub(super) storage: Arc<dyn ChatMediaStorage>,
    pub(super) processor: Arc<dyn ChatMediaProcessor>,
    pub(super) video_chunk_size_bytes: i64,
}

impl ChatMediaService {
    pub fn new(
        repository: Arc<dyn ChatMediaRepository>,
        storage: Arc<dyn ChatMediaStorage>,
    ) -> Self {
        Self {
            repository,
            storage,
            processor: Arc::new(SystemChatMediaProcessor::from_env()),
            video_chunk_size_bytes: configured_video_chunk_size(),
        }
    }

    pub fn with_processor(mut self, processor: Arc<dyn ChatMediaProcessor>) -> Self {
        self.processor = processor;
        self
    }

    #[cfg(test)]
    pub fn with_video_chunk_size(mut self, chunk_size_bytes: i64) -> Self {
        self.video_chunk_size_bytes = chunk_size_bytes.clamp(
            MIN_CHAT_MEDIA_CHUNK_SIZE_BYTES,
            MAX_CHAT_MEDIA_CHUNK_SIZE_BYTES,
        );
        self
    }

    pub fn unavailable() -> Self {
        Self::new(
            Arc::new(super::unavailable::UnavailableChatMediaRepository),
            Arc::new(super::unavailable::UnavailableChatMediaStorage),
        )
    }

    pub async fn initialize_upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        input: ChatMediaInitializeInput,
    ) -> Result<ChatMediaInitialization, ChatMediaError> {
        let conversation_id = validate_identifier(conversation_id)?;
        let input = validate_initialize_input(input)?;
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let media_id = new_id("media");
        let upload_id = new_id("upload");
        let (upload_mode, chunk_size_bytes, total_chunks) =
            self.upload_configuration(input.kind, input.size_bytes)?;
        let created = self
            .repository
            .initialize_upload(
                principal,
                NewChatMediaUpload {
                    source_object_key: format!("chat_media/{conversation_id}/{media_id}/source"),
                    media_id,
                    upload_id,
                    conversation_id: conversation_id.to_string(),
                    client_upload_id: input.client_upload_id.clone(),
                    kind: input.kind,
                    original_filename: input.filename.clone(),
                    declared_content_type: input.content_type.clone(),
                    declared_size_bytes: input.size_bytes,
                    declared_duration_ms: input.duration_ms,
                    upload_mode,
                    chunk_size_bytes,
                    total_chunks,
                    expires_at_unix: now + UPLOAD_RETENTION_SECONDS,
                },
            )
            .await?;
        ensure_idempotent_input_matches(&created, &input)?;
        if !created.created
            && !matches!(
                created.record.status,
                ChatMediaStatus::Pending | ChatMediaStatus::Uploaded
            )
        {
            return Err(ChatMediaError::Conflict);
        }
        self.prepare_initialization(principal, created).await
    }

    pub async fn upload_status(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
    ) -> Result<ChatMediaUploadView, ChatMediaError> {
        let conversation_id = validate_identifier(conversation_id)?;
        let upload_id = validate_identifier(upload_id)?;
        self.upload_view(principal, conversation_id, upload_id, false)
            .await
    }

    pub async fn upload_content(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        content_length: Option<i64>,
        content_type: Option<&str>,
        stream: ChatMediaByteStream,
    ) -> Result<ChatMediaUploadView, ChatMediaError> {
        let conversation_id = validate_identifier(conversation_id)?;
        let upload_id = validate_identifier(upload_id)?;
        let record = self
            .repository
            .upload(principal, conversation_id, upload_id, true)
            .await?;
        if record.upload_mode != ChatMediaUploadMode::Single {
            return Err(ChatMediaError::Conflict);
        }
        if !matches!(
            record.status,
            ChatMediaStatus::Pending | ChatMediaStatus::Uploaded
        ) {
            return Err(ChatMediaError::Conflict);
        }
        if content_length.is_some_and(|value| value != record.declared_size_bytes) {
            return Err(ChatMediaError::InvalidInput);
        }
        if content_type
            .map(normalize_content_type)
            .as_deref()
            .is_none_or(|value| value != record.declared_content_type)
        {
            return Err(ChatMediaError::InvalidInput);
        }
        let stored = self
            .storage
            .put_object(
                &record.source_object_key,
                &record.declared_content_type,
                record.declared_size_bytes,
                stream,
            )
            .await
            .map_err(map_storage_error)?;
        validate_stored_object(&record, &stored)?;
        self.repository
            .mark_uploaded(principal, conversation_id, upload_id, &stored)
            .await
            .map(|record| ChatMediaUploadView::from(&record))
    }

    pub async fn complete_upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
    ) -> Result<ChatMediaUploadView, ChatMediaError> {
        let conversation_id = validate_identifier(conversation_id)?;
        let upload_id = validate_identifier(upload_id)?;
        let record = self
            .repository
            .upload(principal, conversation_id, upload_id, true)
            .await?;
        if record.upload_mode == ChatMediaUploadMode::Chunked {
            return self
                .complete_chunked_upload(principal, conversation_id, upload_id, record)
                .await;
        }
        if matches!(
            record.status,
            ChatMediaStatus::Processing | ChatMediaStatus::Ready
        ) {
            return Ok(ChatMediaUploadView::from(&record));
        }
        if !matches!(
            record.status,
            ChatMediaStatus::Pending | ChatMediaStatus::Uploaded
        ) {
            return Err(ChatMediaError::Conflict);
        }
        let stored = self
            .storage
            .object_metadata(&record.source_object_key)
            .await
            .map_err(map_storage_error)?;
        validate_stored_object(&record, &stored)?;
        self.repository
            .complete_upload(
                principal,
                conversation_id,
                upload_id,
                &stored,
                &new_id("media_job"),
            )
            .await
            .map(|record| ChatMediaUploadView::from(&record))
    }

    pub async fn cancel_upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
    ) -> Result<ChatMediaUploadView, ChatMediaError> {
        let conversation_id = validate_identifier(conversation_id)?;
        let upload_id = validate_identifier(upload_id)?;
        let record = self
            .repository
            .cancel_upload(principal, conversation_id, upload_id)
            .await?;
        if let Some(storage_upload_id) = record.storage_multipart_upload_id.as_deref() {
            match self
                .storage
                .abort_multipart_upload(&record.source_object_key, storage_upload_id)
                .await
            {
                Ok(()) | Err(ChatMediaStorageError::ObjectNotFound) => {}
                Err(error) => return Err(map_storage_error(error)),
            }
        }
        for key in [
            Some(record.source_object_key.as_str()),
            record.processed_object_key.as_deref(),
            record.thumbnail_object_key.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            match self.storage.delete_object(key).await {
                Ok(()) | Err(ChatMediaStorageError::ObjectNotFound) => {}
                Err(error) => return Err(map_storage_error(error)),
            }
        }
        Ok(ChatMediaUploadView::from(&record))
    }

    pub async fn cleanup_orphaned_uploads(&self, limit: usize) -> Result<usize, ChatMediaError> {
        let records = self
            .repository
            .claim_orphaned_uploads(
                OffsetDateTime::now_utc().unix_timestamp(),
                limit.clamp(1, 100),
            )
            .await?;
        let mut cleaned = 0;
        for record in records {
            if let Some(storage_upload_id) = record.storage_multipart_upload_id.as_deref()
                && !matches!(
                    self.storage
                        .abort_multipart_upload(&record.source_object_key, storage_upload_id,)
                        .await,
                    Ok(()) | Err(ChatMediaStorageError::ObjectNotFound)
                )
            {
                self.repository
                    .release_orphan_cleanup(&record.media_id)
                    .await?;
                continue;
            }
            let keys = [
                Some(record.source_object_key.as_str()),
                record.processed_object_key.as_deref(),
                record.thumbnail_object_key.as_deref(),
            ];
            let mut deleted = true;
            for key in keys.into_iter().flatten() {
                if !matches!(
                    self.storage.delete_object(key).await,
                    Ok(()) | Err(ChatMediaStorageError::ObjectNotFound)
                ) {
                    deleted = false;
                }
            }
            if deleted {
                self.repository
                    .mark_orphan_cleaned(&record.media_id)
                    .await?;
                cleaned += 1;
            } else {
                self.repository
                    .release_orphan_cleanup(&record.media_id)
                    .await?;
            }
        }
        Ok(cleaned)
    }

    pub async fn media_access(
        &self,
        principal: &Principal,
        media_id: &str,
        variant: ChatMediaAccessVariant,
    ) -> Result<ChatMediaAccess, ChatMediaError> {
        let media_id = validate_identifier(media_id)?;
        let media = self
            .repository
            .media_for_access(principal, media_id)
            .await?;
        let object_key = match variant {
            ChatMediaAccessVariant::Content => media.processed_object_key.as_deref(),
            ChatMediaAccessVariant::Thumbnail => media.thumbnail_object_key.as_deref(),
        }
        .ok_or(ChatMediaError::NotFound)?;
        match self
            .storage
            .prepare_download(object_key)
            .await
            .map_err(map_storage_error)?
        {
            ChatMediaStorageDownload::DirectGet {
                url,
                expires_at_unix,
            } => Ok(ChatMediaAccess::Redirect {
                url,
                expires_at_unix,
            }),
            ChatMediaStorageDownload::LocalProxy => self
                .storage
                .read_object(object_key)
                .await
                .map(|content| ChatMediaAccess::Local { content })
                .map_err(map_storage_error),
        }
    }

    pub async fn media_stream_access(
        &self,
        principal: &Principal,
        media_id: &str,
        variant: ChatMediaAccessVariant,
        range: ChatMediaRangeRequest,
    ) -> Result<ChatMediaStreamAccess, ChatMediaError> {
        let media_id = validate_identifier(media_id)?;
        let media = self
            .repository
            .media_for_access(principal, media_id)
            .await?;
        let object_key = match variant {
            ChatMediaAccessVariant::Content => media.processed_object_key.as_deref(),
            ChatMediaAccessVariant::Thumbnail => media.thumbnail_object_key.as_deref(),
        }
        .ok_or(ChatMediaError::NotFound)?;
        match self
            .storage
            .prepare_download(object_key)
            .await
            .map_err(map_storage_error)?
        {
            ChatMediaStorageDownload::DirectGet {
                url,
                expires_at_unix,
            } => Ok(ChatMediaStreamAccess::Redirect {
                url,
                expires_at_unix,
            }),
            ChatMediaStorageDownload::LocalProxy => self
                .storage
                .stream_object(object_key, range)
                .await
                .map(|content| ChatMediaStreamAccess::Local { content })
                .map_err(map_storage_error),
        }
    }

    pub async fn issue_playback_ticket(
        &self,
        principal: &Principal,
        media_id: &str,
    ) -> Result<(String, i64), ChatMediaError> {
        let media_id = validate_identifier(media_id)?;
        let mut bytes = [0_u8; 32];
        rand::fill(&mut bytes);
        let ticket = URL_SAFE_NO_PAD.encode(bytes);
        let expires_at_unix =
            OffsetDateTime::now_utc().unix_timestamp() + PLAYBACK_TICKET_TTL_SECONDS;
        self.repository
            .create_access_ticket(principal, media_id, &ticket_hash(&ticket), expires_at_unix)
            .await?;
        Ok((ticket, expires_at_unix))
    }

    pub async fn media_stream_access_with_ticket(
        &self,
        media_id: &str,
        ticket: &str,
        variant: ChatMediaAccessVariant,
        range: ChatMediaRangeRequest,
    ) -> Result<ChatMediaStreamAccess, ChatMediaError> {
        let media_id = validate_identifier(media_id)?;
        if ticket.trim().is_empty() {
            return Err(ChatMediaError::Forbidden);
        }
        let media = self
            .repository
            .media_for_access_ticket(media_id, &ticket_hash(ticket))
            .await?;
        self.stream_media_record(&media, variant, range).await
    }

    async fn stream_media_record(
        &self,
        media: &ChatMediaUploadRecord,
        variant: ChatMediaAccessVariant,
        range: ChatMediaRangeRequest,
    ) -> Result<ChatMediaStreamAccess, ChatMediaError> {
        let object_key = match variant {
            ChatMediaAccessVariant::Content => media.processed_object_key.as_deref(),
            ChatMediaAccessVariant::Thumbnail => media.thumbnail_object_key.as_deref(),
        }
        .ok_or(ChatMediaError::NotFound)?;
        match self
            .storage
            .prepare_download(object_key)
            .await
            .map_err(map_storage_error)?
        {
            ChatMediaStorageDownload::DirectGet {
                url,
                expires_at_unix,
            } => Ok(ChatMediaStreamAccess::Redirect {
                url,
                expires_at_unix,
            }),
            ChatMediaStorageDownload::LocalProxy => self
                .storage
                .stream_object(object_key, range)
                .await
                .map(|content| ChatMediaStreamAccess::Local { content })
                .map_err(map_storage_error),
        }
    }
}

include!("service_validation.rs");
