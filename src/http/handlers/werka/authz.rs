use axum::Json;
use axum::http::{HeaderMap, StatusCode};

use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::core::authz::Capability;
use crate::http::handlers::auth::{ErrorResponse, bearer_token};

pub(super) async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

pub(super) async fn require_werka(
    state: &AppState,
    principal: &Principal,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if state
        .admin
        .principal_has_capability(principal, Capability::WerkaAccess)
        .await
    {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse { error: "forbidden" }),
        ))
    }
}

fn unauthorized() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: "unauthorized",
        }),
    )
}
