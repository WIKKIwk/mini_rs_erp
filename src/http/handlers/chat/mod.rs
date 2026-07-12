mod auth;
mod conversations;
mod devices;
mod directory;
mod realtime;

pub use conversations::{conversation_messages, conversations, create_dm, mark_read};
pub use devices::device_token;
pub use directory::directory;
pub use realtime::{live, socket_ticket};

use axum::Json;
use axum::http::StatusCode;
use serde::Serialize;

use crate::core::chat::ChatError;

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
        ChatError::Unavailable => http_error(StatusCode::SERVICE_UNAVAILABLE, "chat_unavailable"),
        ChatError::StoreFailed => {
            http_error(StatusCode::INTERNAL_SERVER_ERROR, "chat_store_failed")
        }
    }
}
