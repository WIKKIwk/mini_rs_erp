use super::*;

use crate::telegram::{TelegramBotSettingsUpdate, TelegramError, TelegramInviteRequest};

pub async fn settings(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminSettingsRead,
            Capability::AdminSettingsManage,
        ],
    )
    .await?;
    match method {
        Method::GET => {
            require_capability(&state, &principal, Capability::AdminSettingsRead).await?;
            state
                .telegram
                .admin_overview()
                .await
                .map(json_response)
                .map_err(telegram_error)
        }
        Method::PUT => {
            require_capability(&state, &principal, Capability::AdminSettingsManage).await?;
            let input: TelegramBotSettingsUpdate = parse_json(&body)?;
            state
                .telegram
                .update_bot_settings(input)
                .await
                .map(json_response)
                .map_err(telegram_error)
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn invite(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminSettingsManage).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: TelegramInviteRequest = parse_json(&body)?;
    state
        .telegram
        .create_invite(input)
        .await
        .map(json_response)
        .map_err(telegram_error)
}

fn telegram_error(error: TelegramError) -> AdminError {
    match error {
        TelegramError::BotUsernameRequired => bad_request("telegram bot username is required"),
        TelegramError::BotTokenRequired => bad_request("telegram bot token is not configured"),
        TelegramError::InviteTokenRequired => bad_request("telegram invite token is required"),
        TelegramError::UserIdRequired => bad_request("telegram user id is required"),
        TelegramError::InviteNotFound => bad_request("telegram invite not found"),
        TelegramError::InviteAlreadyUsed => bad_request("telegram invite already used"),
        TelegramError::InviteExpired => bad_request("telegram invite expired"),
        TelegramError::Transport(_) => server_error("telegram transport failed"),
        TelegramError::UserAccountNotConfigured => {
            bad_request("telegram user account API credentials are not configured")
        }
        TelegramError::UserAccountNotAuthorized => {
            bad_request("telegram user account is not connected")
        }
        TelegramError::UserAccountInvalidCode => bad_request("telegram login code is invalid"),
        TelegramError::UserAccountSignUpRequired => {
            bad_request("telegram account registration is required")
        }
        TelegramError::UserAccountAccountMismatch => {
            bad_request("telegram account does not match the bot user")
        }
        TelegramError::UserAccountGroupNotWritable => {
            bad_request("telegram selected group is not writable")
        }
        TelegramError::UserAccount(_) => server_error("telegram user account operation failed"),
        TelegramError::Store => server_error("telegram store failed"),
        TelegramError::OrderCatalogNotConfigured => {
            bad_request("telegram order catalog is not configured")
        }
        TelegramError::OrderCatalog(_) => server_error("telegram order catalog failed"),
    }
}
