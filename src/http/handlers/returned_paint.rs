use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::core::authz::Capability;
use crate::core::returned_paint::{
    ReturnedPaintError, ReturnedPaintRequest, ReturnedPaintRequestCreate,
};
use crate::http::handlers::auth::{ErrorResponse, bearer_token};

#[derive(Debug, Deserialize)]
pub struct ReturnedPaintListQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ReturnedPaintListResponse {
    items: Vec<ReturnedPaintRequest>,
    has_more: bool,
}

pub async fn requests(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ReturnedPaintListQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let principal = authorize(&state, &headers).await?;
    match method {
        Method::POST => {
            require_capability(
                &state,
                &principal,
                Capability::ReturnedPaintRequestCreate,
            )
            .await?;
            let input = serde_json::from_slice::<ReturnedPaintRequestCreate>(&body)
                .map_err(|_| bad_request("invalid json"))?;
            let request = state
                .returned_paint
                .create(input, &principal)
                .await
                .map_err(returned_paint_error)?;
            Ok(Json(
                serde_json::to_value(request).map_err(|_| server_error())?,
            ))
        }
        Method::GET => {
            require_capability(
                &state,
                &principal,
                Capability::ReturnedPaintRequestRead,
            )
            .await?;
            let limit = query.limit.unwrap_or(20).clamp(1, 100);
            let offset = query.offset.unwrap_or(0);
            let mut items = state
                .returned_paint
                .list(limit.saturating_add(1), offset)
                .await
                .map_err(returned_paint_error)?;
            let has_more = items.len() > limit;
            items.truncate(limit);
            Ok(Json(
                serde_json::to_value(ReturnedPaintListResponse { items, has_more })
                    .map_err(|_| server_error())?,
            ))
        }
        _ => Err(method_not_allowed()),
    }
}

async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<ErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

async fn require_capability(
    state: &AppState,
    principal: &Principal,
    capability: Capability,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if state
        .admin
        .principal_has_capability(principal, capability)
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

fn returned_paint_error(
    error: ReturnedPaintError,
) -> (StatusCode, Json<ErrorResponse>) {
    match error {
        ReturnedPaintError::MissingOrderId
        | ReturnedPaintError::MissingApparatus
        | ReturnedPaintError::MissingItems
        | ReturnedPaintError::InvalidUsage
        | ReturnedPaintError::InvalidCategory
        | ReturnedPaintError::MissingItemName
        | ReturnedPaintError::MissingValues
        | ReturnedPaintError::InvalidValue
        | ReturnedPaintError::NegativeFinalValue => {
            bad_request("returned paint request is invalid")
        }
        ReturnedPaintError::StoreFailed => server_error(),
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

fn bad_request(error: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (StatusCode::BAD_REQUEST, Json(ErrorResponse { error }))
}

fn server_error() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "returned paint request failed",
        }),
    )
}

fn method_not_allowed() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(ErrorResponse {
            error: "method not allowed",
        }),
    )
}
