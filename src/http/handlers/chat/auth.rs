use axum::http::HeaderMap;

use super::{ChatHttpError, http_error};
use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::http::handlers::auth::bearer_token;

pub(super) async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(String, Principal), ChatHttpError> {
    let token = bearer_token(headers).ok_or_else(|| {
        http_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "authentication_required",
        )
    })?;
    let principal = state.sessions.get(&token).await.map_err(|_| {
        http_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "authentication_required",
        )
    })?;
    Ok((token, principal))
}
