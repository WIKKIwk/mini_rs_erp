use std::pin::Pin;
use std::path::Path;

use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;

use super::{
    ChatMediaCreateResult, ChatMediaMultipartUpload, ChatMediaProcessedContent,
    ChatMediaProcessedFiles, ChatMediaProcessingWorkItem, ChatMediaReadyInput,
    ChatMediaStorageDownload, ChatMediaStorageObject, ChatMediaStoragePart,
    ChatMediaStorageUpload, ChatMediaStoredContent, ChatMediaUploadRecord,
    ChatMediaUploadedChunk, NewChatMediaUpload, NewChatMediaUploadedChunk,
};
use crate::core::auth::models::Principal;

pub type ChatMediaByteStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, ChatMediaStorageError>> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatMediaRangeRequest {
    Full,
    From {
        start_byte: i64,
        end_byte_inclusive: Option<i64>,
    },
    Suffix {
        length_bytes: i64,
    },
}

pub struct ChatMediaStoredStream {
    pub stream: ChatMediaByteStream,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub total_size_bytes: i64,
    pub start_byte: i64,
    pub end_byte_inclusive: i64,
    pub partial: bool,
}

impl ChatMediaStoredStream {
    pub fn content_length(&self) -> i64 {
        self.end_byte_inclusive - self.start_byte + 1
    }
}

pub enum ChatMediaStreamAccess {
    Local { content: ChatMediaStoredStream },
    Redirect { url: String, expires_at_unix: i64 },
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum ChatMediaError {
    #[error("chat media is unavailable")]
    Unavailable,
    #[error("chat media input is invalid")]
    InvalidInput,
    #[error("chat media is too large")]
    TooLarge,
    #[error("chat video exceeds the duration limit")]
    DurationTooLong,
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

    async fn set_multipart_upload_id(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        storage_upload_id: &str,
    ) -> Result<ChatMediaUploadRecord, ChatMediaError>;

    async fn uploaded_chunks(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        require_can_post: bool,
    ) -> Result<Vec<ChatMediaUploadedChunk>, ChatMediaError>;

    async fn record_uploaded_chunk(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        chunk: NewChatMediaUploadedChunk,
    ) -> Result<ChatMediaUploadedChunk, ChatMediaError>;

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

    async fn begin_multipart_upload(
        &self,
        _object_key: &str,
        _content_type: &str,
    ) -> Result<ChatMediaMultipartUpload, ChatMediaStorageError> {
        Err(ChatMediaStorageError::Unavailable)
    }

    async fn put_multipart_part(
        &self,
        _object_key: &str,
        _storage_upload_id: &str,
        _part_number: i32,
        _content: Bytes,
    ) -> Result<ChatMediaStoragePart, ChatMediaStorageError> {
        Err(ChatMediaStorageError::Unavailable)
    }

    async fn complete_multipart_upload(
        &self,
        _object_key: &str,
        _content_type: &str,
        _storage_upload_id: &str,
        _expected_size_bytes: i64,
        _parts: &[ChatMediaStoragePart],
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        Err(ChatMediaStorageError::Unavailable)
    }

    async fn abort_multipart_upload(
        &self,
        _object_key: &str,
        _storage_upload_id: &str,
    ) -> Result<(), ChatMediaStorageError> {
        Err(ChatMediaStorageError::Unavailable)
    }

    async fn object_metadata(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError>;

    async fn delete_object(&self, object_key: &str) -> Result<(), ChatMediaStorageError>;

    async fn read_object(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStoredContent, ChatMediaStorageError>;

    async fn stream_object(
        &self,
        object_key: &str,
        range: ChatMediaRangeRequest,
    ) -> Result<ChatMediaStoredStream, ChatMediaStorageError> {
        let content = self.read_object(object_key).await?;
        let total_size_bytes = i64::try_from(content.bytes.len())
            .map_err(|_| ChatMediaStorageError::SizeMismatch)?;
        let (start_byte, end_byte_inclusive, partial) =
            resolve_media_range(range, total_size_bytes)?;
        let start = usize::try_from(start_byte)
            .map_err(|_| ChatMediaStorageError::SizeMismatch)?;
        let end = usize::try_from(end_byte_inclusive)
            .map_err(|_| ChatMediaStorageError::SizeMismatch)?;
        let bytes = content.bytes.slice(start..=end);
        Ok(ChatMediaStoredStream {
            stream: Box::pin(async_stream::stream! {
                yield Ok(bytes);
            }),
            content_type: content.content_type,
            etag: content.etag,
            total_size_bytes,
            start_byte,
            end_byte_inclusive,
            partial,
        })
    }

    async fn download_object_to_file(
        &self,
        object_key: &str,
        destination: &Path,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let content = self.read_object(object_key).await?;
        tokio::fs::write(destination, &content.bytes)
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        Ok(ChatMediaStorageObject {
            size_bytes: i64::try_from(content.bytes.len())
                .map_err(|_| ChatMediaStorageError::SizeMismatch)?,
            content_type: content.content_type,
            etag: content.etag,
        })
    }

    async fn put_private_object(
        &self,
        object_key: &str,
        content_type: &str,
        content: Bytes,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError>;

    async fn put_private_file(
        &self,
        object_key: &str,
        content_type: &str,
        source: &Path,
    ) -> Result<ChatMediaStorageObject, ChatMediaStorageError> {
        let content = tokio::fs::read(source)
            .await
            .map_err(|_| ChatMediaStorageError::OperationFailed)?;
        self.put_private_object(object_key, content_type, Bytes::from(content))
            .await
    }

    async fn prepare_download(
        &self,
        object_key: &str,
    ) -> Result<ChatMediaStorageDownload, ChatMediaStorageError>;
}

pub fn resolve_media_range(
    range: ChatMediaRangeRequest,
    total_size_bytes: i64,
) -> Result<(i64, i64, bool), ChatMediaStorageError> {
    if total_size_bytes <= 0 {
        return Err(ChatMediaStorageError::SizeMismatch);
    }
    let full = (0, total_size_bytes - 1, false);
    let resolved = match range {
        ChatMediaRangeRequest::Full => full,
        ChatMediaRangeRequest::From {
            start_byte,
            end_byte_inclusive,
        } if start_byte >= 0 && start_byte < total_size_bytes => {
            let end = end_byte_inclusive
                .unwrap_or(total_size_bytes - 1)
                .min(total_size_bytes - 1);
            if end < start_byte {
                full
            } else {
                (start_byte, end, true)
            }
        }
        ChatMediaRangeRequest::Suffix { length_bytes } if length_bytes > 0 => {
            let length = length_bytes.min(total_size_bytes);
            (total_size_bytes - length, total_size_bytes - 1, true)
        }
        _ => full,
    };
    Ok(resolved)
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum ChatMediaProcessingError {
    #[error("media processing is unavailable")]
    Unavailable,
    #[error("uploaded media content is invalid")]
    InvalidContent,
    #[error("uploaded video exceeds the duration limit")]
    DurationTooLong,
    #[error("uploaded video exceeds the resolution limit")]
    ResolutionTooLarge,
    #[error("uploaded video exceeds the frame-rate limit")]
    FrameRateTooHigh,
    #[error("processed video exceeds the output size limit")]
    ProcessedTooLarge,
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

    async fn process_file(
        &self,
        media: &ChatMediaUploadRecord,
        source_path: &Path,
        content_path: &Path,
        thumbnail_path: &Path,
    ) -> Result<ChatMediaProcessedFiles, ChatMediaProcessingError> {
        let source = tokio::fs::read(source_path)
            .await
            .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?;
        let processed = self.process(media, Bytes::from(source)).await?;
        tokio::fs::write(content_path, &processed.content)
            .await
            .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?;
        tokio::fs::write(thumbnail_path, &processed.thumbnail)
            .await
            .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?;
        Ok(ChatMediaProcessedFiles {
            content_path: content_path.to_path_buf(),
            content_type: processed.content_type,
            thumbnail_path: thumbnail_path.to_path_buf(),
            thumbnail_content_type: processed.thumbnail_content_type,
            width_pixels: processed.width_pixels,
            height_pixels: processed.height_pixels,
            duration_ms: processed.duration_ms,
            frame_rate_milli: processed.frame_rate_milli,
            video_codec: processed.video_codec,
            audio_codec: processed.audio_codec,
        })
    }
}
