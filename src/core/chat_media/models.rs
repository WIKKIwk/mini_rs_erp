use std::collections::BTreeMap;
use std::path::PathBuf;

use bytes::Bytes;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMediaKind {
    Image,
    Video,
}

impl ChatMediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMediaUploadMode {
    Single,
    Chunked,
}

impl ChatMediaUploadMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Chunked => "chunked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatMediaStatus {
    Pending,
    Uploaded,
    Processing,
    Ready,
    Failed,
    Cancelled,
}

impl ChatMediaStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Uploaded => "uploaded",
            Self::Processing => "processing",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMediaUploadRecord {
    pub media_id: String,
    pub upload_id: String,
    pub conversation_id: String,
    pub uploader_principal_id: String,
    pub client_upload_id: String,
    pub kind: ChatMediaKind,
    pub status: ChatMediaStatus,
    pub original_filename: String,
    pub declared_content_type: String,
    pub declared_size_bytes: i64,
    pub declared_duration_ms: Option<i64>,
    pub upload_mode: ChatMediaUploadMode,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMediaUploadView {
    pub media_id: String,
    pub upload_id: String,
    pub conversation_id: String,
    pub client_upload_id: String,
    pub kind: ChatMediaKind,
    pub status: ChatMediaStatus,
    pub original_filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub upload_mode: ChatMediaUploadMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_size_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_chunks: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uploaded_chunks: Vec<ChatMediaUploadedChunk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_size_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width_pixels: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_pixels: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processed_duration_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processed_size_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_rate_milli: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_codec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_codec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    pub expires_at_unix: i64,
    pub created_at_unix: i64,
    pub updated_at_unix: i64,
}

impl From<&ChatMediaUploadRecord> for ChatMediaUploadView {
    fn from(record: &ChatMediaUploadRecord) -> Self {
        Self {
            media_id: record.media_id.clone(),
            upload_id: record.upload_id.clone(),
            conversation_id: record.conversation_id.clone(),
            client_upload_id: record.client_upload_id.clone(),
            kind: record.kind,
            status: record.status,
            original_filename: record.original_filename.clone(),
            content_type: record.declared_content_type.clone(),
            size_bytes: record.declared_size_bytes,
            upload_mode: record.upload_mode,
            chunk_size_bytes: record.chunk_size_bytes,
            total_chunks: record.total_chunks,
            uploaded_chunks: Vec::new(),
            duration_ms: record.declared_duration_ms,
            actual_size_bytes: record.actual_size_bytes,
            width_pixels: record.width_pixels,
            height_pixels: record.height_pixels,
            processed_duration_ms: record.duration_ms,
            processed_size_bytes: record.processed_size_bytes,
            frame_rate_milli: record.frame_rate_milli,
            video_codec: record.video_codec.clone(),
            audio_codec: record.audio_codec.clone(),
            error_code: record.error_code.clone(),
            expires_at_unix: record.expires_at_unix,
            created_at_unix: record.created_at_unix,
            updated_at_unix: record.updated_at_unix,
        }
    }
}

impl ChatMediaUploadView {
    pub fn with_uploaded_chunks(mut self, chunks: Vec<ChatMediaUploadedChunk>) -> Self {
        self.uploaded_chunks = chunks;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMediaInitializeInput {
    pub client_upload_id: String,
    pub kind: ChatMediaKind,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewChatMediaUpload {
    pub media_id: String,
    pub upload_id: String,
    pub conversation_id: String,
    pub client_upload_id: String,
    pub kind: ChatMediaKind,
    pub original_filename: String,
    pub declared_content_type: String,
    pub declared_size_bytes: i64,
    pub declared_duration_ms: Option<i64>,
    pub upload_mode: ChatMediaUploadMode,
    pub chunk_size_bytes: Option<i64>,
    pub total_chunks: Option<i32>,
    pub source_object_key: String,
    pub expires_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMediaCreateResult {
    pub record: ChatMediaUploadRecord,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMediaUploadInstruction {
    pub strategy: String,
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub expires_at_unix: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_size_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_chunks: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMediaInitialization {
    pub media: ChatMediaUploadView,
    pub upload: ChatMediaUploadInstruction,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMediaChunkUploadResult {
    pub media: ChatMediaUploadView,
    pub chunk: ChatMediaUploadedChunk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMediaUploadedChunk {
    pub chunk_index: i32,
    pub offset_bytes: i64,
    pub size_bytes: i64,
    pub storage_part_etag: String,
    pub uploaded_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewChatMediaUploadedChunk {
    pub chunk_index: i32,
    pub offset_bytes: i64,
    pub size_bytes: i64,
    pub storage_part_etag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatMediaStorageUpload {
    LocalProxy,
    DirectPut {
        url: String,
        headers: BTreeMap<String, String>,
        expires_at_unix: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMediaMultipartUpload {
    pub storage_upload_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMediaStoragePart {
    pub part_number: i32,
    pub size_bytes: i64,
    pub etag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMediaStorageObject {
    pub size_bytes: i64,
    pub content_type: Option<String>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMediaStoredContent {
    pub bytes: Bytes,
    pub content_type: Option<String>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatMediaStorageDownload {
    LocalProxy,
    DirectGet {
        url: String,
        expires_at_unix: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatMediaAccessVariant {
    Content,
    Thumbnail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatMediaAccess {
    Local {
        content: ChatMediaStoredContent,
    },
    Redirect {
        url: String,
        expires_at_unix: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMediaProcessedContent {
    pub content: Bytes,
    pub content_type: String,
    pub thumbnail: Bytes,
    pub thumbnail_content_type: String,
    pub width_pixels: i32,
    pub height_pixels: i32,
    pub duration_ms: Option<i64>,
    pub frame_rate_milli: Option<i32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMediaProcessedFiles {
    pub content_path: PathBuf,
    pub content_type: String,
    pub thumbnail_path: PathBuf,
    pub thumbnail_content_type: String,
    pub width_pixels: i32,
    pub height_pixels: i32,
    pub duration_ms: Option<i64>,
    pub frame_rate_milli: Option<i32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMediaProcessingWorkItem {
    pub job_id: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub media: ChatMediaUploadRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMediaReadyInput {
    pub processed_object_key: String,
    pub processed_content_type: String,
    pub processed_size_bytes: i64,
    pub processed_etag: Option<String>,
    pub thumbnail_object_key: String,
    pub width_pixels: i32,
    pub height_pixels: i32,
    pub duration_ms: Option<i64>,
    pub frame_rate_milli: Option<i32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessageAttachment {
    pub attachment_id: String,
    pub message_id: String,
    pub conversation_id: String,
    pub media_id: String,
    pub ordinal: i16,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMediaProcessingJob {
    pub job_id: String,
    pub media_id: String,
    pub job_type: String,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub available_at_unix: i64,
    pub locked_until_unix: Option<i64>,
    pub last_error: Option<String>,
    pub created_at_unix: i64,
    pub updated_at_unix: i64,
}
