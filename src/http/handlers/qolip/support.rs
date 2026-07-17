use axum::Json;
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::core::authz::Capability;
use crate::core::gscale::GscaleServiceError;
use crate::core::qolip::{QolipBlock, QolipError};
use crate::http::handlers::auth::bearer_token;

pub(super) async fn accessible_qolip_block(
    state: &AppState,
    principal: &Principal,
    block: &str,
) -> Result<Option<QolipBlock>, (StatusCode, Json<QolipErrorResponse>)> {
    let block = block.trim();
    if block.is_empty() {
        return Ok(None);
    }
    if state
        .admin
        .principal_has_capability(principal, Capability::AdminAccess)
        .await
    {
        return Ok(None);
    }
    let assigned = state
        .qolip
        .assigned_blocks(principal)
        .await
        .map_err(qolip_error)?;
    assigned
        .into_iter()
        .find(|item| item.name.trim().eq_ignore_ascii_case(block))
        .ok_or_else(forbidden)
        .map(Some)
}

pub(super) async fn accessible_qolip_warehouse(
    state: &AppState,
    principal: &Principal,
    warehouse: &str,
) -> Result<String, (StatusCode, Json<QolipErrorResponse>)> {
    let warehouse = warehouse.trim();
    if state
        .admin
        .principal_has_capability(principal, Capability::AdminAccess)
        .await
    {
        if warehouse.is_empty() {
            return Err(bad_request("warehouse_required"));
        }
        return Ok(warehouse.to_string());
    }
    let assigned = state
        .qolip
        .assigned_warehouses(principal)
        .await
        .map_err(qolip_error)?;
    if warehouse.is_empty() && assigned.len() == 1 {
        return Ok(assigned[0].clone());
    }
    assigned
        .into_iter()
        .find(|item| item.trim().eq_ignore_ascii_case(warehouse))
        .ok_or_else(forbidden)
}

#[derive(Debug, Deserialize)]
pub struct QolipSearchQuery {
    pub(super) q: Option<String>,
    pub(super) block: Option<String>,
    pub(super) limit: Option<usize>,
    pub(super) with_qolip: Option<bool>,
    pub(super) with_qolip_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct QolipBlockUpsert {
    #[serde(default)]
    pub(super) warehouse: String,
    #[serde(default)]
    pub(super) block: String,
}

#[derive(Debug, Deserialize)]
pub struct QolipCheckoutsQuery {
    #[serde(default)]
    pub(super) block: Option<String>,
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct QolipCellQrLookupQuery {
    #[serde(default)]
    pub(super) qr: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct QolipCellQrPrintRequest {
    #[serde(default)]
    pub(super) block: String,
    #[serde(default)]
    pub(super) warehouse: String,
    #[serde(default)]
    pub(super) row_letter: String,
    #[serde(default)]
    pub(super) column_number: Option<i32>,
    #[serde(default)]
    pub(super) driver_url: String,
    #[serde(default)]
    pub(super) printer: String,
    #[serde(default)]
    pub(super) print_mode: String,
    #[serde(default)]
    pub(super) print_count: u32,
    #[serde(default)]
    pub(super) print_transport: String,
}

#[derive(Debug, Deserialize)]
pub struct QolipCodeQrPrintRequest {
    #[serde(default)]
    pub(super) qolip_code: String,
    #[serde(default)]
    pub(super) driver_url: String,
    #[serde(default)]
    pub(super) printer: String,
    #[serde(default)]
    pub(super) print_mode: String,
    #[serde(default)]
    pub(super) print_count: u32,
    #[serde(default)]
    pub(super) print_transport: String,
}

pub(super) async fn authenticated_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, (StatusCode, Json<QolipErrorResponse>)> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

pub(super) async fn ensure_qolip_access(
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

pub(super) fn qolip_error(error: QolipError) -> (StatusCode, Json<QolipErrorResponse>) {
    match error {
        QolipError::StoreFailed => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(QolipErrorResponse::new("qolip_store_failed")),
        ),
        QolipError::MissingBlock => bad_request("block_required"),
        QolipError::MissingItem => bad_request("item_required"),
        QolipError::MissingItemGroup => bad_request("item_group_required"),
        QolipError::MissingQolipCode => bad_request("qolip_code_required"),
        QolipError::QolipCodeNotFound => bad_request("qolip_code_not_found"),
        QolipError::QolipCodeMismatch => bad_request("qolip_code_mismatch"),
        QolipError::InvalidSize => bad_request("size_required"),
        QolipError::InvalidQuantity => bad_request("quantity_required"),
        QolipError::InvalidLocation => bad_request("location_invalid"),
        QolipError::LocationNotFound => bad_request("location_not_found"),
        QolipError::MissingWorker => bad_request("worker_required"),
        QolipError::WorkerNotFound => bad_request("worker_not_found"),
        QolipError::InsufficientStock => (
            StatusCode::CONFLICT,
            Json(QolipErrorResponse::new("insufficient_stock")),
        ),
        QolipError::CheckoutNotFound => bad_request("checkout_not_found"),
        QolipError::CheckoutNotReturnable => bad_request("checkout_not_returnable"),
        QolipError::CellQrNotFound => bad_request("cell_qr_not_found"),
        QolipError::LocationIdentityMismatch => (
            StatusCode::CONFLICT,
            Json(QolipErrorResponse::new("location_identity_mismatch")),
        ),
    }
}

pub(super) fn gscale_print_error(
    error: GscaleServiceError,
) -> (StatusCode, Json<QolipErrorResponse>) {
    let status = match error {
        GscaleServiceError::InvalidInput(_) => StatusCode::BAD_REQUEST,
        GscaleServiceError::NotConfigured(_) => StatusCode::SERVICE_UNAVAILABLE,
        GscaleServiceError::PrintFailed { .. } => StatusCode::FAILED_DEPENDENCY,
        GscaleServiceError::EpcGenerationFailed
        | GscaleServiceError::StoreWrite(_)
        | GscaleServiceError::SubmitFailed(_) => StatusCode::FAILED_DEPENDENCY,
    };
    (status, Json(QolipErrorResponse::new(error.code())))
}

fn unauthorized() -> (StatusCode, Json<QolipErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(QolipErrorResponse::new("unauthorized")),
    )
}

pub(super) fn forbidden() -> (StatusCode, Json<QolipErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(QolipErrorResponse::new("forbidden")),
    )
}

pub(super) fn method_not_allowed() -> (StatusCode, Json<QolipErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(QolipErrorResponse::new("method_not_allowed")),
    )
}

pub(super) fn bad_request(error: &'static str) -> (StatusCode, Json<QolipErrorResponse>) {
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
