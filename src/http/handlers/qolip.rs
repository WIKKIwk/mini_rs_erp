use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::core::auth::models::Principal;
use crate::core::authz::Capability;
use crate::core::gscale::{GscaleServiceError, ProgressLabelPrintRequest};
use crate::core::qolip::{
    QolipBlock, QolipCellQrInput, QolipCheckoutCreate, QolipCheckoutReturn, QolipError,
    QolipLocationMove, QolipLocationUpsert, QolipProductSpecUpsert,
};
use crate::core::warehouses::WarehouseUpsert;
use crate::http::handlers::auth::bearer_token;

pub async fn blocks(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::GET && method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    match method {
        Method::GET => {
            let is_admin = state
                .admin
                .principal_has_capability(&principal, Capability::AdminAccess)
                .await;
            let blocks = state
                .qolip
                .blocks_for_principal(&principal, is_admin)
                .await
                .map_err(qolip_error)?;
            let warehouses = state
                .qolip
                .warehouses_for_principal(&principal, is_admin)
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "warehouses": warehouses,
                "blocks": blocks,
            })))
        }
        Method::POST => {
            let input: QolipBlockUpsert =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let block = input.block.trim();
            if block.is_empty() {
                return Err(bad_request("block_required"));
            }
            let parent = accessible_qolip_warehouse(&state, &principal, &input.warehouse).await?;
            let saved = state
                .warehouses
                .upsert_warehouse(WarehouseUpsert {
                    warehouse: block.to_string(),
                    company: String::new(),
                    is_group: false,
                    parent_warehouse: parent.clone(),
                })
                .await
                .map_err(|_| qolip_error(QolipError::StoreFailed))?;
            let block = QolipBlock {
                name: saved.warehouse,
                warehouse: saved.parent_warehouse,
            };
            Ok(Json(serde_json::json!({
                "ok": true,
                "block": block,
            })))
        }
        _ => Err(method_not_allowed()),
    }
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
        .products(
            query.q.as_deref().unwrap_or(""),
            query.limit.unwrap_or(50),
            query.with_qolip.unwrap_or(false) || query.with_qolip_only.unwrap_or(false),
        )
        .await
        .map_err(qolip_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "products": products,
    })))
}

pub async fn product_specs(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let input: QolipProductSpecUpsert =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let spec = state
        .qolip
        .upsert_product_spec(input, &principal)
        .await
        .map_err(qolip_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "product": {
            "code": spec.item_code,
            "name": spec.item_name,
            "item_group": spec.item_group,
            "qolip_code": spec.qolip_code,
            "size": spec.size,
            "has_qolip_spec": true,
        },
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
            let mut block_query = query.block.as_deref().unwrap_or("").trim().to_string();
            if block_query.is_empty() {
                let assigned = state
                    .qolip
                    .assigned_blocks(&principal)
                    .await
                    .map_err(qolip_error)?;
                if assigned.len() == 1 {
                    block_query = assigned[0].name.clone();
                }
            }
            let block = match accessible_qolip_block(&state, &principal, &block_query).await? {
                Some(block) => block.name,
                None => block_query,
            };
            let locations = state.qolip.locations(&block).await.map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "locations": locations,
            })))
        }
        Method::POST => {
            let mut input: QolipLocationUpsert =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            if let Some(block) = accessible_qolip_block(&state, &principal, &input.block).await? {
                input.block = block.name;
                input.warehouse = block.warehouse;
            }
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

pub async fn workers(
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
    let workers = state
        .workers
        .workers(query.q.as_deref().unwrap_or(""), query.limit.unwrap_or(100))
        .await
        .map_err(|_| qolip_error(QolipError::StoreFailed))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "workers": workers,
    })))
}

pub async fn checkouts(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipCheckoutsQuery>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    match method {
        Method::GET => {
            if let Some(block) = query
                .block
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                let _ = accessible_qolip_block(&state, &principal, block).await?;
            }
            let is_admin = state
                .admin
                .principal_has_capability(&principal, Capability::AdminAccess)
                .await;
            let checkouts = state
                .qolip
                .checkouts(
                    &principal,
                    is_admin,
                    query
                        .block
                        .as_deref()
                        .filter(|value| !value.trim().is_empty()),
                    query.status.as_deref().unwrap_or("open"),
                    query.limit.unwrap_or(50),
                )
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "checkouts": checkouts,
            })))
        }
        Method::POST => {
            let input: QolipCheckoutCreate =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let location = state
                .qolip
                .location_by_id(&input.location_id)
                .await
                .map_err(qolip_error)?
                .ok_or_else(|| bad_request("location_not_found"))?;
            let _ = accessible_qolip_block(&state, &principal, &location.block).await?;
            let worker_id = input.worker_id.trim();
            if worker_id.is_empty() {
                return Err(bad_request("worker_required"));
            }
            let workers = state
                .workers
                .workers_by_ids(&[worker_id.to_string()])
                .await
                .map_err(|_| qolip_error(QolipError::StoreFailed))?;
            let Some(worker) = workers.into_iter().next() else {
                return Err(bad_request("worker_not_found"));
            };
            let checkout = state
                .qolip
                .issue_checkout_from_location(
                    location,
                    input.quantity,
                    &worker.id,
                    &worker.name,
                    &principal,
                )
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "checkout": checkout,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn checkout_return(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let input: QolipCheckoutReturn =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let checkout_id = input.checkout_id.trim();
    if checkout_id.is_empty() {
        return Err(bad_request("checkout_required"));
    }
    let checkout = state
        .qolip
        .checkout_by_id(checkout_id)
        .await
        .map_err(qolip_error)?
        .ok_or_else(|| bad_request("checkout_not_found"))?;
    let _ = accessible_qolip_block(&state, &principal, &checkout.block).await?;
    let returned = state
        .qolip
        .return_checkout(input)
        .await
        .map_err(qolip_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "checkout": returned,
    })))
}

pub async fn location_move(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let input: QolipLocationMove =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let location = state
        .qolip
        .location_by_id(&input.location_id)
        .await
        .map_err(qolip_error)?
        .ok_or_else(|| bad_request("location_not_found"))?;
    let _ = accessible_qolip_block(&state, &principal, &location.block).await?;
    let saved = state
        .qolip
        .move_location(input, &principal)
        .await
        .map_err(qolip_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "location": saved,
    })))
}

pub async fn cell_qr(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<QolipCellQrLookupQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let qr = query.qr.as_deref().unwrap_or("").trim();
    if qr.is_empty() {
        return Err(bad_request("qr_required"));
    }
    let is_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    if !is_admin
        && state
            .qolip
            .assigned_blocks(&principal)
            .await
            .map_err(qolip_error)?
            .is_empty()
    {
        return Err(forbidden());
    }
    let cell_qr = state
        .qolip
        .resolve_cell_qr(qr, &principal)
        .await
        .map_err(qolip_error)?
        .ok_or_else(|| bad_request("cell_qr_not_found"))?;
    let _ = accessible_qolip_block(&state, &principal, &cell_qr.block).await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "cell_qr": cell_qr,
    })))
}

pub async fn cell_qr_print(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    let input: QolipCellQrPrintRequest =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let mut cell_input = QolipCellQrInput {
        block: input.block.clone(),
        warehouse: input.warehouse.clone(),
        row_letter: input.row_letter.clone(),
        column_number: input.column_number,
    };
    if let Some(block) = accessible_qolip_block(&state, &principal, &input.block).await? {
        cell_input.block = block.name;
        cell_input.warehouse = block.warehouse;
    }
    let cell_qr = state
        .qolip
        .cell_qr(cell_input, &principal)
        .await
        .map_err(qolip_error)?;
    let print = state
        .gscale
        .print_progress_label(ProgressLabelPrintRequest {
            driver_url: input.driver_url,
            qr_payload: cell_qr.qr_payload.clone(),
            item_code: cell_qr.qr_payload.clone(),
            item_name: cell_qr.location_label.clone(),
            executor_name: principal.display_name.trim().to_string(),
            printer: input.printer,
            print_mode: input.print_mode,
            label_kind: "qr_center".to_string(),
            gross_qty: 1.0,
            progress_qty: 1.0,
            unit: "dona".to_string(),
            progress_unit: "dona".to_string(),
            print_count: input.print_count,
        })
        .await
        .map_err(gscale_print_error)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "cell_qr": cell_qr,
        "print": print,
    })))
}

async fn accessible_qolip_block(
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

async fn accessible_qolip_warehouse(
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
    q: Option<String>,
    block: Option<String>,
    limit: Option<usize>,
    with_qolip: Option<bool>,
    with_qolip_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct QolipBlockUpsert {
    #[serde(default)]
    warehouse: String,
    #[serde(default)]
    block: String,
}

#[derive(Debug, Deserialize)]
pub struct QolipCheckoutsQuery {
    #[serde(default)]
    block: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct QolipCellQrLookupQuery {
    #[serde(default)]
    qr: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct QolipCellQrPrintRequest {
    #[serde(default)]
    block: String,
    #[serde(default)]
    warehouse: String,
    #[serde(default)]
    row_letter: String,
    #[serde(default)]
    column_number: Option<i32>,
    #[serde(default)]
    driver_url: String,
    #[serde(default)]
    printer: String,
    #[serde(default)]
    print_mode: String,
    #[serde(default)]
    print_count: u32,
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

fn gscale_print_error(error: GscaleServiceError) -> (StatusCode, Json<QolipErrorResponse>) {
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
