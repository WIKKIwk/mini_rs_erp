use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::Deserialize;
use serde::Serialize;

use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::core::authz::Capability;
use crate::core::push::models::PushTokenRegisterRequest;
use crate::core::push::ports::PushServiceError;
use crate::http::handlers::auth::{ErrorResponse, bearer_token};

pub async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
    method: Method,
    Query(query): Query<PushTokenDeleteQuery>,
    body: Bytes,
) -> Result<Json<OkResponse>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    require_push_role(&state, &principal).await?;

    match method {
        Method::POST => {
            let request: PushTokenRegisterRequest =
                serde_json::from_slice(&body).map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ErrorResponse {
                            error: "invalid json",
                        }),
                    )
                })?;
            if request.token.trim().is_empty() {
                return Err(token_required());
            }
            state
                .push
                .register(&principal, &request.token, &request.platform)
                .await
                .map_err(register_error)?;
            Ok(Json(OkResponse { ok: true }))
        }
        Method::DELETE => {
            let token = query.token.as_deref().unwrap_or("").trim();
            if token.is_empty() {
                return Err(token_required());
            }
            state
                .push
                .delete(&principal, token)
                .await
                .map_err(delete_error)?;
            Ok(Json(OkResponse { ok: true }))
        }
        _ => Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(ErrorResponse {
                error: "method not allowed",
            }),
        )),
    }
}

async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

async fn require_push_role(
    state: &AppState,
    principal: &Principal,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if state
        .admin
        .principal_has_capability(principal, Capability::PushTokenManage)
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

fn register_error(error: PushServiceError) -> (StatusCode, Json<ErrorResponse>) {
    match error {
        PushServiceError::TokenRequired => token_required(),
        PushServiceError::StoreFailed => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "push token save failed",
            }),
        ),
    }
}

fn delete_error(error: PushServiceError) -> (StatusCode, Json<ErrorResponse>) {
    match error {
        PushServiceError::TokenRequired => token_required(),
        PushServiceError::StoreFailed => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "push token delete failed",
            }),
        ),
    }
}

fn token_required() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: "token is required",
        }),
    )
}

fn unauthorized() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: "unauthorized",
        }),
    )
}

#[derive(Debug, Deserialize)]
pub struct PushTokenDeleteQuery {
    pub token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OkResponse {
    pub ok: bool,
}
