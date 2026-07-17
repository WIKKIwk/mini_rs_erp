use std::sync::Arc;
use std::time::Duration;

use time::OffsetDateTime;

use super::{
    ChatMediaByteStream, ChatMediaCreateResult, ChatMediaError, ChatMediaInitialization,
    ChatMediaInitializeInput, ChatMediaKind, ChatMediaRepository, ChatMediaStatus,
    ChatMediaStorage, ChatMediaStorageError, ChatMediaStorageObject, ChatMediaStorageUpload,
    ChatMediaUploadInstruction, ChatMediaUploadRecord, ChatMediaUploadView, NewChatMediaUpload,
};
use crate::core::auth::models::Principal;

pub const MAX_CHAT_IMAGE_SIZE_BYTES: i64 = 15 * 1024 * 1024;
pub const MAX_CHAT_VIDEO_SIZE_BYTES: i64 = 75 * 1024 * 1024;
pub const MAX_CHAT_VIDEO_DURATION_MS: i64 = 120_000;

const UPLOAD_RETENTION_SECONDS: i64 = 24 * 60 * 60;
const CLEANUP_INTERVAL_SECONDS: u64 = 60 * 60;

#[derive(Clone)]
pub struct ChatMediaService {
    repository: Arc<dyn ChatMediaRepository>,
    storage: Arc<dyn ChatMediaStorage>,
}

impl ChatMediaService {
    pub fn new(
        repository: Arc<dyn ChatMediaRepository>,
        storage: Arc<dyn ChatMediaStorage>,
    ) -> Self {
        Self {
            repository,
            storage,
        }
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
        let created = self
            .repository
            .initialize_upload(
                principal,
                NewChatMediaUpload {
                    source_object_key: format!(
                        "chat_media/{conversation_id}/{media_id}/source"
                    ),
                    media_id,
                    upload_id,
                    conversation_id: conversation_id.to_string(),
                    client_upload_id: input.client_upload_id.clone(),
                    kind: input.kind,
                    original_filename: input.filename.clone(),
                    declared_content_type: input.content_type.clone(),
                    declared_size_bytes: input.size_bytes,
                    declared_duration_ms: input.duration_ms,
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
        let storage_upload = self
            .storage
            .prepare_upload(
                &created.record.source_object_key,
                &created.record.declared_content_type,
                created.record.declared_size_bytes,
            )
            .await
            .map_err(map_storage_error)?;
        let instruction = upload_instruction(&created.record, storage_upload);
        Ok(ChatMediaInitialization {
            media: ChatMediaUploadView::from(&created.record),
            upload: instruction,
            created: created.created,
        })
    }

    pub async fn upload_status(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
    ) -> Result<ChatMediaUploadView, ChatMediaError> {
        let conversation_id = validate_identifier(conversation_id)?;
        let upload_id = validate_identifier(upload_id)?;
        self.repository
            .upload(principal, conversation_id, upload_id, false)
            .await
            .map(|record| ChatMediaUploadView::from(&record))
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
        if !matches!(record.status, ChatMediaStatus::Pending | ChatMediaStatus::Uploaded) {
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
        if matches!(record.status, ChatMediaStatus::Processing | ChatMediaStatus::Ready) {
            return Ok(ChatMediaUploadView::from(&record));
        }
        if !matches!(record.status, ChatMediaStatus::Pending | ChatMediaStatus::Uploaded) {
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
        match self.storage.delete_object(&record.source_object_key).await {
            Ok(()) | Err(ChatMediaStorageError::ObjectNotFound) => {
                Ok(ChatMediaUploadView::from(&record))
            }
            Err(error) => Err(map_storage_error(error)),
        }
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
            match self.storage.delete_object(&record.source_object_key).await {
                Ok(()) | Err(ChatMediaStorageError::ObjectNotFound) => {
                    self.repository
                        .mark_orphan_cleaned(&record.media_id)
                        .await?;
                    cleaned += 1;
                }
                Err(_) => {
                    self.repository
                        .release_orphan_cleanup(&record.media_id)
                        .await?;
                }
            }
        }
        Ok(cleaned)
    }

    pub fn start_cleanup_worker(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let service = self.clone();
        handle.spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(CLEANUP_INTERVAL_SECONDS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if let Err(error) = service.cleanup_orphaned_uploads(100).await {
                    tracing::warn!(%error, "chat media orphan cleanup failed");
                }
            }
        });
    }
}

fn validate_initialize_input(
    mut input: ChatMediaInitializeInput,
) -> Result<ChatMediaInitializeInput, ChatMediaError> {
    input.client_upload_id = validate_identifier(&input.client_upload_id)?.to_string();
    input.filename = input.filename.trim().to_string();
    input.content_type = normalize_content_type(&input.content_type);
    if input.filename.is_empty()
        || input.filename.chars().count() > 255
        || input.filename.chars().any(char::is_control)
        || input.size_bytes <= 0
    {
        return Err(ChatMediaError::InvalidInput);
    }
    let (maximum_size, allowed_types) = match input.kind {
        ChatMediaKind::Image => (
            MAX_CHAT_IMAGE_SIZE_BYTES,
            &["image/jpeg", "image/png", "image/webp"][..],
        ),
        ChatMediaKind::Video => (
            MAX_CHAT_VIDEO_SIZE_BYTES,
            &["video/mp4", "video/quicktime", "video/webm"][..],
        ),
    };
    if input.size_bytes > maximum_size {
        return Err(ChatMediaError::TooLarge);
    }
    if !allowed_types.contains(&input.content_type.as_str()) {
        return Err(ChatMediaError::InvalidInput);
    }
    match (input.kind, input.duration_ms) {
        (ChatMediaKind::Image, Some(_)) => return Err(ChatMediaError::InvalidInput),
        (ChatMediaKind::Video, Some(duration))
            if !(1..=MAX_CHAT_VIDEO_DURATION_MS).contains(&duration) =>
        {
            return Err(ChatMediaError::InvalidInput);
        }
        _ => {}
    }
    Ok(input)
}

fn validate_identifier(value: &str) -> Result<&str, ChatMediaError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(ChatMediaError::InvalidInput);
    }
    Ok(value)
}

fn normalize_content_type(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "image/jpg" => "image/jpeg".to_string(),
        value => value.to_string(),
    }
}

fn ensure_idempotent_input_matches(
    created: &ChatMediaCreateResult,
    input: &ChatMediaInitializeInput,
) -> Result<(), ChatMediaError> {
    let record = &created.record;
    if record.client_upload_id != input.client_upload_id
        || record.kind != input.kind
        || record.original_filename != input.filename
        || record.declared_content_type != input.content_type
        || record.declared_size_bytes != input.size_bytes
        || record.declared_duration_ms != input.duration_ms
    {
        return Err(ChatMediaError::Conflict);
    }
    Ok(())
}

fn validate_stored_object(
    record: &ChatMediaUploadRecord,
    stored: &ChatMediaStorageObject,
) -> Result<(), ChatMediaError> {
    if stored.size_bytes != record.declared_size_bytes {
        return Err(ChatMediaError::InvalidInput);
    }
    let maximum = match record.kind {
        ChatMediaKind::Image => MAX_CHAT_IMAGE_SIZE_BYTES,
        ChatMediaKind::Video => MAX_CHAT_VIDEO_SIZE_BYTES,
    };
    if stored.size_bytes > maximum {
        return Err(ChatMediaError::TooLarge);
    }
    Ok(())
}

fn upload_instruction(
    record: &ChatMediaUploadRecord,
    upload: ChatMediaStorageUpload,
) -> ChatMediaUploadInstruction {
    match upload {
        ChatMediaStorageUpload::LocalProxy => ChatMediaUploadInstruction {
            strategy: "local_proxy".to_string(),
            method: "PUT".to_string(),
            url: format!(
                "/v1/mobile/chat/conversations/{}/media/uploads/{}/content",
                record.conversation_id, record.upload_id
            ),
            headers: [("content-type".to_string(), record.declared_content_type.clone())]
                .into_iter()
                .collect(),
            expires_at_unix: record.expires_at_unix,
        },
        ChatMediaStorageUpload::DirectPut {
            url,
            headers,
            expires_at_unix,
        } => ChatMediaUploadInstruction {
            strategy: "direct_put".to_string(),
            method: "PUT".to_string(),
            url,
            headers,
            expires_at_unix,
        },
    }
}

fn map_storage_error(error: ChatMediaStorageError) -> ChatMediaError {
    match error {
        ChatMediaStorageError::Unavailable => ChatMediaError::Unavailable,
        ChatMediaStorageError::ObjectNotFound => ChatMediaError::NotFound,
        ChatMediaStorageError::SizeMismatch => ChatMediaError::InvalidInput,
        ChatMediaStorageError::DirectUploadRequired => ChatMediaError::Conflict,
        ChatMediaStorageError::InvalidObjectKey => ChatMediaError::StoreFailed,
        ChatMediaStorageError::OperationFailed => ChatMediaError::StorageFailed,
    }
}

fn new_id(prefix: &str) -> String {
    let bytes: [u8; 16] = rand::random();
    format!("{prefix}_{}", data_encoding::HEXLOWER.encode(&bytes))
}
