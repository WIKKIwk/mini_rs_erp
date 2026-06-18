use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::core::authz::Capability;
use crate::core::qolip::{QolipError, QolipLocationUpsert};
use crate::http::handlers::auth::bearer_token;

pub async fn blocks(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let blocks = state
        .qolip
        .assigned_blocks(&principal)
        .await
        .map_err(qolip_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "blocks": blocks,
    })))
}

pub async fn products(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipSearchQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let products = state
        .qolip
        .products(query.q.as_deref().unwrap_or(""), query.limit.unwrap_or(50))
        .await
        .map_err(qolip_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "products": products,
    })))
}

pub async fn locations(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipSearchQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    match method {
        Method::GET => {
            let locations = state
                .qolip
                .locations(query.block.as_deref().unwrap_or(""))
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "locations": locations,
            })))
        }
        Method::POST => {
            let input: QolipLocationUpsert =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let saved = state
                .qolip
                .upsert_location(input, &principal)
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "location": saved,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

#[derive(Debug, Deserialize)]
pub struct QolipSearchQuery {
    q: Option<String>,
    block: Option<String>,
    limit: Option<usize>,
}

async fn authenticated_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<QolipErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

async fn ensure_qolip_access(
    state: &AppState,
    principal: &Principal,
) -> Result<(), (StatusCode, Json<QolipErrorResponse>)> {
    if state
        .admin
        .principal_has_capability(principal, Capability::QolipManage)
        .await
        || state
            .admin
            .principal_has_capability(principal, Capability::AdminAccess)
            .await
    {
        Ok(())
    } else {
        Err(forbidden())
    }
}

fn qolip_error(error: QolipError) -> (StatusCode, Json<QolipErrorResponse>) {
    match error {
        QolipError::StoreFailed => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(QolipErrorResponse::new("qolip_store_failed")),
        ),
        QolipError::MissingBlock => bad_request("block_required"),
        QolipError::MissingItem => bad_request("item_required"),
        QolipError::MissingQolipCode => bad_request("qolip_code_required"),
        QolipError::InvalidSize => bad_request("size_required"),
        QolipError::InvalidQuantity => bad_request("quantity_required"),
        QolipError::InvalidLocation => bad_request("location_invalid"),
    }
}

fn unauthorized() -> (StatusCode, Json<QolipErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(QolipErrorResponse::new("unauthorized")),
    )
}

fn forbidden() -> (StatusCode, Json<QolipErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(QolipErrorResponse::new("forbidden")),
    )
}

fn method_not_allowed() -> (StatusCode, Json<QolipErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(QolipErrorResponse::new("method_not_allowed")),
    )
}

fn bad_request(error: &'static str) -> (StatusCode, Json<QolipErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(QolipErrorResponse::new(error)),
    )
}

#[derive(Debug, Serialize)]
pub struct QolipErrorResponse {
    ok: bool,
    error: &'static str,
}

impl QolipErrorResponse {
    fn new(error: &'static str) -> Self {
        Self { ok: false, error }
    }
}
