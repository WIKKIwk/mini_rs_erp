use crate::core::chat_media::{
    ChatMediaError, ChatMediaKind, ChatMediaProcessingWorkItem, ChatMediaStatus,
    ChatMediaUploadMode, ChatMediaUploadRecord, ChatMediaUploadedChunk,
};

#[derive(sqlx::FromRow)]
pub(super) struct ChatMediaRow {
    pub media_id: String,
    pub upload_id: String,
    pub conversation_id: String,
    pub uploader_principal_id: String,
    pub client_upload_id: String,
    pub media_kind: String,
    pub upload_status: String,
    pub original_filename: String,
    pub declared_content_type: String,
    pub declared_size_bytes: i64,
    pub declared_duration_ms: Option<i64>,
    pub upload_mode: String,
    pub chunk_size_bytes: Option<i64>,
    pub total_chunks: Option<i32>,
    pub storage_multipart_upload_id: Option<String>,
    pub source_object_key: String,
    pub actual_size_bytes: Option<i64>,
    pub storage_etag: Option<String>,
    pub detected_content_type: Option<String>,
    pub processed_object_key: Option<String>,
    pub thumbnail_object_key: Option<String>,
    pub processed_content_type: Option<String>,
    pub processed_size_bytes: Option<i64>,
    pub processed_etag: Option<String>,
    pub width_pixels: Option<i32>,
    pub height_pixels: Option<i32>,
    pub duration_ms: Option<i64>,
    pub frame_rate_milli: Option<i32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub error_code: Option<String>,
    pub expires_at_unix: i64,
    pub created_at_unix: i64,
    pub updated_at_unix: i64,
}

#[derive(sqlx::FromRow)]
pub(super) struct ChatMediaChunkRow {
    pub chunk_index: i32,
    pub offset_bytes: i64,
    pub size_bytes: i64,
    pub storage_part_etag: String,
    pub uploaded_at_unix: i64,
}

impl ChatMediaChunkRow {
    pub fn into_model(self) -> ChatMediaUploadedChunk {
        ChatMediaUploadedChunk {
            chunk_index: self.chunk_index,
            offset_bytes: self.offset_bytes,
            size_bytes: self.size_bytes,
            storage_part_etag: self.storage_part_etag,
            uploaded_at_unix: self.uploaded_at_unix,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct ChatMediaWorkRow {
    pub job_id: String,
    pub attempts: i32,
    pub max_attempts: i32,
    #[sqlx(flatten)]
    pub media: ChatMediaRow,
}

impl ChatMediaWorkRow {
    pub fn into_model(self) -> Result<ChatMediaProcessingWorkItem, ChatMediaError> {
        Ok(ChatMediaProcessingWorkItem {
            job_id: self.job_id,
            attempts: self.attempts,
            max_attempts: self.max_attempts,
            media: self.media.into_model()?,
        })
    }
}

impl ChatMediaRow {
    pub fn into_model(self) -> Result<ChatMediaUploadRecord, ChatMediaError> {
        Ok(ChatMediaUploadRecord {
            media_id: self.media_id,
            upload_id: self.upload_id,
            conversation_id: self.conversation_id,
            uploader_principal_id: self.uploader_principal_id,
            client_upload_id: self.client_upload_id,
            kind: parse_kind(&self.media_kind)?,
            status: parse_status(&self.upload_status)?,
            original_filename: self.original_filename,
            declared_content_type: self.declared_content_type,
            declared_size_bytes: self.declared_size_bytes,
            declared_duration_ms: self.declared_duration_ms,
            upload_mode: parse_upload_mode(&self.upload_mode)?,
            chunk_size_bytes: self.chunk_size_bytes,
            total_chunks: self.total_chunks,
            storage_multipart_upload_id: self.storage_multipart_upload_id,
            source_object_key: self.source_object_key,
            actual_size_bytes: self.actual_size_bytes,
            storage_etag: self.storage_etag,
            detected_content_type: self.detected_content_type,
            processed_object_key: self.processed_object_key,
            thumbnail_object_key: self.thumbnail_object_key,
            processed_content_type: self.processed_content_type,
            processed_size_bytes: self.processed_size_bytes,
            processed_etag: self.processed_etag,
            width_pixels: self.width_pixels,
            height_pixels: self.height_pixels,
            duration_ms: self.duration_ms,
            frame_rate_milli: self.frame_rate_milli,
            video_codec: self.video_codec,
            audio_codec: self.audio_codec,
            error_code: self.error_code,
            expires_at_unix: self.expires_at_unix,
            created_at_unix: self.created_at_unix,
            updated_at_unix: self.updated_at_unix,
        })
    }
}

fn parse_upload_mode(value: &str) -> Result<ChatMediaUploadMode, ChatMediaError> {
    match value {
        "single" => Ok(ChatMediaUploadMode::Single),
        "chunked" => Ok(ChatMediaUploadMode::Chunked),
        _ => Err(ChatMediaError::StoreFailed),
    }
}

fn parse_kind(value: &str) -> Result<ChatMediaKind, ChatMediaError> {
    match value {
        "image" => Ok(ChatMediaKind::Image),
        "video" => Ok(ChatMediaKind::Video),
        "audio" => Ok(ChatMediaKind::Audio),
        _ => Err(ChatMediaError::StoreFailed),
    }
}

fn parse_status(value: &str) -> Result<ChatMediaStatus, ChatMediaError> {
    match value {
        "pending" => Ok(ChatMediaStatus::Pending),
        "uploaded" => Ok(ChatMediaStatus::Uploaded),
        "processing" => Ok(ChatMediaStatus::Processing),
        "ready" => Ok(ChatMediaStatus::Ready),
        "failed" => Ok(ChatMediaStatus::Failed),
        "cancelled" => Ok(ChatMediaStatus::Cancelled),
        _ => Err(ChatMediaError::StoreFailed),
    }
}
