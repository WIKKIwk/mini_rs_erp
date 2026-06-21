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
    QolipBlock, QolipCellQrInput, QolipError, QolipLocationUpsert, QolipProductSpecUpsert,
};
use crate::core::warehouses::WarehouseUpsert;
use crate::http::handlers::auth::bearer_token;

pub async fn blocks(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::GET {
        if method != Method::POST {
            return Err(method_not_allowed());
        }
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    match method {
        Method::GET => {
            let blocks = state
                .qolip
                .assigned_blocks(&principal)
                .await
                .map_err(qolip_error)?;
            let warehouses = assigned_qolip_warehouses(&state, &principal)
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
            let block = query.block.as_deref().unwrap_or("").trim();
            let block = match accessible_qolip_block(&state, &principal, block).await? {
                Some(block) => block.name,
                None => block.to_string(),
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

async fn assigned_qolip_warehouses(
    state: &AppState,
    principal: &Principal,
) -> Result<Vec<String>, QolipError> {
    state.qolip.assigned_warehouses(principal).await
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
