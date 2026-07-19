mod auth;
mod conversations;
mod devices;
mod directory;
mod media;
mod realtime;

pub use conversations::{
    conversation_messages, conversations, create_dm, mark_delivered, mark_read,
};
pub use devices::device_token;
pub use directory::directory;
pub use media::{
    media_access, media_playback_ticket, media_upload, media_upload_chunk, media_upload_complete,
    media_upload_content, media_uploads,
};
pub use realtime::{live, socket_ticket, sync};

use axum::Json;
use axum::http::StatusCode;
use serde::Serialize;

use crate::core::chat::ChatError;
use crate::core::chat_media::ChatMediaError;

type ChatHttpError = (StatusCode, Json<ChatErrorResponse>);

#[derive(Debug, Serialize)]
pub struct ChatErrorResponse {
    error: String,
}

fn http_error(status: StatusCode, error: impl Into<String>) -> ChatHttpError {
    (
        status,
        Json(ChatErrorResponse {
            error: error.into(),
        }),
    )
}

fn map_chat_error(error: ChatError) -> ChatHttpError {
    match error {
        ChatError::InvalidInput => http_error(StatusCode::BAD_REQUEST, "chat_input_invalid"),
        ChatError::NotFound => http_error(StatusCode::NOT_FOUND, "chat_not_found"),
        ChatError::Forbidden => http_error(StatusCode::FORBIDDEN, "chat_forbidden"),
        ChatError::Conflict => http_error(StatusCode::CONFLICT, "chat_state_conflict"),
        ChatError::Unavailable => http_error(StatusCode::SERVICE_UNAVAILABLE, "chat_unavailable"),
        ChatError::StoreFailed => {
            http_error(StatusCode::INTERNAL_SERVER_ERROR, "chat_store_failed")
        }
    }
}

fn map_chat_media_error(error: ChatMediaError) -> ChatHttpError {
    match error {
        ChatMediaError::InvalidInput => {
            http_error(StatusCode::BAD_REQUEST, "chat_media_input_invalid")
        }
        ChatMediaError::TooLarge => {
            http_error(StatusCode::PAYLOAD_TOO_LARGE, "chat_media_too_large")
        }
        ChatMediaError::DurationTooLong => {
            http_error(StatusCode::UNPROCESSABLE_ENTITY, "video_duration_too_long")
        }
        ChatMediaError::AudioDurationTooLong => {
            http_error(StatusCode::UNPROCESSABLE_ENTITY, "audio_duration_too_long")
        }
        ChatMediaError::NotFound => {
            http_error(StatusCode::NOT_FOUND, "chat_media_upload_not_found")
        }
        ChatMediaError::Forbidden => http_error(StatusCode::FORBIDDEN, "chat_media_forbidden"),
        ChatMediaError::Conflict => http_error(StatusCode::CONFLICT, "chat_media_state_conflict"),
        ChatMediaError::Unavailable => {
            http_error(StatusCode::SERVICE_UNAVAILABLE, "chat_media_unavailable")
        }
        ChatMediaError::StoreFailed => {
            http_error(StatusCode::INTERNAL_SERVER_ERROR, "chat_media_store_failed")
        }
        ChatMediaError::StorageFailed => {
            http_error(StatusCode::BAD_GATEWAY, "chat_media_storage_failed")
        }
    }
}
