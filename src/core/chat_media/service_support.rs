use super::service::{
    DEFAULT_CHAT_VIDEO_CHUNK_SIZE_BYTES, MAX_CHAT_MEDIA_CHUNK_SIZE_BYTES,
    MIN_CHAT_MEDIA_CHUNK_SIZE_BYTES,
};
use super::{
    ChatMediaError, ChatMediaStorageError, ChatMediaStorageUpload,
    ChatMediaUploadInstruction, ChatMediaUploadRecord,
};

pub(super) fn upload_instruction(
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
            headers: [(
                "content-type".to_string(),
                record.declared_content_type.clone(),
            )]
            .into_iter()
            .collect(),
            expires_at_unix: record.expires_at_unix,
            chunk_size_bytes: None,
            total_chunks: None,
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
            chunk_size_bytes: None,
            total_chunks: None,
        },
    }
}

pub(super) fn map_storage_error(error: ChatMediaStorageError) -> ChatMediaError {
    match error {
        ChatMediaStorageError::Unavailable => ChatMediaError::Unavailable,
        ChatMediaStorageError::ObjectNotFound => ChatMediaError::NotFound,
        ChatMediaStorageError::SizeMismatch => ChatMediaError::InvalidInput,
        ChatMediaStorageError::DirectUploadRequired => ChatMediaError::Conflict,
        ChatMediaStorageError::InvalidObjectKey => ChatMediaError::StoreFailed,
        ChatMediaStorageError::OperationFailed => ChatMediaError::StorageFailed,
    }
}

pub(super) fn new_id(prefix: &str) -> String {
    let bytes: [u8; 16] = rand::random();
    format!("{prefix}_{}", data_encoding::HEXLOWER.encode(&bytes))
}

pub(super) fn configured_video_chunk_size() -> i64 {
    std::env::var("MOBILE_CHAT_MEDIA_CHUNK_SIZE_BYTES")
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_CHAT_VIDEO_CHUNK_SIZE_BYTES)
        .clamp(
            MIN_CHAT_MEDIA_CHUNK_SIZE_BYTES,
            MAX_CHAT_MEDIA_CHUNK_SIZE_BYTES,
        )
}
