use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode};
use axum::response::IntoResponse;

use crate::app::AppState;
use crate::http::handlers::auth::ErrorResponse;
use crate::telegram::{TelegramError, TelegramStartRequest};

pub async fn start(
    State(state): State<AppState>,
    method: Method,
    body: Bytes,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    if method != Method::POST {
        return Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(ErrorResponse {
                error: "method not allowed",
            }),
        ));
    }
    let input: TelegramStartRequest = serde_json::from_slice(&body).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid json",
            }),
        )
    })?;
    state
        .telegram
        .register_start(input)
        .await
        .map(Json)
        .map_err(start_error)
}

fn start_error(error: TelegramError) -> (StatusCode, Json<ErrorResponse>) {
    let is_store_error = matches!(
        &error,
        TelegramError::Store | TelegramError::OrderCatalog(_)
    );
    let message = match error {
        TelegramError::InviteTokenRequired => "telegram invite token is required",
        TelegramError::UserIdRequired => "telegram user id is required",
        TelegramError::InviteNotFound => "telegram invite not found",
        TelegramError::InviteAlreadyUsed => "telegram invite already used",
        TelegramError::InviteExpired => "telegram invite expired",
        TelegramError::BotUsernameRequired => "telegram bot username is required",
        TelegramError::BotTokenRequired => "telegram bot token is not configured",
        TelegramError::Transport(_) => "telegram transport failed",
        TelegramError::UserAccountNotConfigured => {
            "telegram user account API credentials are not configured"
        }
        TelegramError::UserAccountNotAuthorized => "telegram user account is not connected",
        TelegramError::UserAccountInvalidCode => "telegram login code is invalid",
        TelegramError::UserAccountSignUpRequired => "telegram account registration is required",
        TelegramError::UserAccountAccountMismatch => {
            "telegram account does not match the bot user"
        }
        TelegramError::UserAccountGroupNotWritable => "telegram selected group is not writable",
        TelegramError::UserAccount(_) => "telegram user account operation failed",
        TelegramError::Store => "telegram store failed",
        TelegramError::OrderCatalogNotConfigured => "telegram order catalog is not configured",
        TelegramError::OrderCatalog(_) => "telegram order catalog failed",
    };
    let status = if is_store_error {
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::BAD_REQUEST
    };
    (status, Json(ErrorResponse { error: message }))
}
