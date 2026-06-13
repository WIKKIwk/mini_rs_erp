use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::core::admin::ports::AdminPortError;
use crate::core::auth::models::Principal;
use crate::core::authz::Capability;
use crate::core::gscale::{GscaleServiceError, MaterialReceiptPrintRequest};
use crate::http::handlers::auth::{ErrorResponse, bearer_token};

pub async fn items(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<GscaleItemsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<GscaleErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    let can_read_gscale = state
        .admin
        .principal_has_capability(&principal, Capability::GscaleCatalogRead)
        .await;
    let can_manage_rezka = state
        .admin
        .principal_has_capability(&principal, Capability::RezkaSplitManage)
        .await;
    if !can_read_gscale && !can_manage_rezka {
        return Err(forbidden());
    }
    let items = state
        .admin
        .items_page_by_group(
            query.group.as_deref().unwrap_or(""),
            query.q.as_deref().unwrap_or(""),
            positive_int(query.limit.as_deref(), 80).min(200),
            optional_offset(query.offset.as_deref()),
        )
        .await
        .map_err(admin_read_error)?;
    Ok(Json(
        serde_json::to_value(items).unwrap_or_else(|_| serde_json::json!([])),
    ))
}

pub async fn material_receipt_print(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<GscaleErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    if !state
        .admin
        .principal_has_capability(&principal, Capability::GscalePrint)
        .await
    {
        return Err(forbidden());
    }
    let request: MaterialReceiptPrintRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json", "invalid json"))?;
    let response = state
        .gscale
        .print_material_receipt_driver_first(request)
        .await
        .map_err(gscale_error)?;
    Ok(Json(
        serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({"ok": false})),
    ))
}

async fn authenticated_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<GscaleErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

fn gscale_error(error: GscaleServiceError) -> (StatusCode, Json<GscaleErrorResponse>) {
    let status = match error {
        GscaleServiceError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        GscaleServiceError::NotConfigured(_) => StatusCode::SERVICE_UNAVAILABLE,
        GscaleServiceError::EpcGenerationFailed => StatusCode::INTERNAL_SERVER_ERROR,
        GscaleServiceError::StoreWrite(_)
        | GscaleServiceError::PrintFailed { .. }
        | GscaleServiceError::SubmitFailed(_) => StatusCode::FAILED_DEPENDENCY,
    };
    (
        status,
        Json(GscaleErrorResponse {
            ok: false,
            error: error.code(),
            detail: error.to_string(),
        }),
    )
}

fn unauthorized() -> (StatusCode, Json<GscaleErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(GscaleErrorResponse::new("unauthorized", "unauthorized")),
    )
}

fn forbidden() -> (StatusCode, Json<GscaleErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(GscaleErrorResponse::new("forbidden", "forbidden")),
    )
}

fn bad_request(
    error: &'static str,
    detail: &'static str,
) -> (StatusCode, Json<GscaleErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(GscaleErrorResponse::new(error, detail)),
    )
}

fn method_not_allowed() -> (StatusCode, Json<GscaleErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(GscaleErrorResponse::new(
            "method_not_allowed",
            "method not allowed",
        )),
    )
}

#[derive(Debug, Serialize)]
pub struct GscaleErrorResponse {
    pub ok: bool,
    pub error: &'static str,
    pub detail: String,
}

impl GscaleErrorResponse {
    fn new(error: &'static str, detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            error,
            detail: detail.into(),
        }
    }
}

fn admin_read_error(error: AdminPortError) -> (StatusCode, Json<GscaleErrorResponse>) {
    let status = match error {
        AdminPortError::NotFound => StatusCode::NOT_FOUND,
        #[cfg(test)]
        AdminPortError::PermissionDenied => StatusCode::FORBIDDEN,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(GscaleErrorResponse::new(
            "catalog_read_failed",
            "catalog read failed",
        )),
    )
}

fn positive_int(value: Option<&str>, default: usize) -> usize {
    match value.unwrap_or("").trim().parse::<usize>() {
        Ok(value) if value > 0 => value,
        _ => default,
    }
}

fn optional_offset(value: Option<&str>) -> usize {
    value
        .unwrap_or("")
        .trim()
        .parse::<isize>()
        .ok()
        .filter(|value| *value >= 0)
        .unwrap_or(0) as usize
}

#[derive(Debug, Deserialize)]
pub struct GscaleItemsQuery {
    pub q: Option<String>,
    pub group: Option<String>,
    pub limit: Option<String>,
    pub offset: Option<String>,
}

#[allow(dead_code)]
fn _keeps_error_response_compatible(_response: ErrorResponse) {}
