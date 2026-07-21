use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, Method, StatusCode};

use crate::app::AppState;
use crate::core::authz::Capability;
use crate::core::gscale::ProgressLabelPrintRequest;
use crate::core::qolip::{
    QolipBlock, QolipCellQrInput, QolipCheckoutCreate, QolipCheckoutReturn, QolipError,
    QolipLocationMove, QolipLocationUpsert, QolipProductSpecDelete, QolipProductSpecUpsert,
};
use crate::core::warehouses::{WarehouseDeleteRequest, WarehouseUpsert};

mod support;

use self::support::*;
pub use self::support::{
    QolipBlockUpdate, QolipBlockUpsert, QolipCellQrLookupQuery, QolipCellQrPrintRequest,
    QolipCheckoutsQuery, QolipCodeQrPrintRequest, QolipErrorResponse, QolipSearchQuery,
};

pub async fn blocks(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<QolipErrorResponse>)> {
    if method != Method::GET
        && method != Method::POST
        && method != Method::PUT
        && method != Method::DELETE
    {
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
                "supports_cross_block_move": true,
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
        Method::PUT => {
            let input: QolipBlockUpdate =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let new_block = input.new_block.trim();
            if new_block.is_empty() {
                return Err(bad_request("block_required"));
            }
            let is_admin = state
                .admin
                .principal_has_capability(&principal, Capability::AdminAccess)
                .await;
            let current = managed_qolip_block(&state, &principal, &input.block, is_admin).await?;
            let warehouse = accessible_qolip_warehouse(&state, &principal, &input.warehouse).await?;
            if !current
                .warehouse
                .trim()
                .eq_ignore_ascii_case(warehouse.trim())
            {
                return Err(forbidden());
            }

            if !current.name.trim().eq_ignore_ascii_case(new_block) {
                let already_exists = state
                    .warehouses
                    .warehouses(new_block, "", 200)
                    .await
                    .map_err(|_| qolip_error(QolipError::StoreFailed))?
                    .into_iter()
                    .any(|item| item.warehouse.trim().eq_ignore_ascii_case(new_block));
                if already_exists {
                    return Err(conflict("block_exists"));
                }
            }

            let saved = state
                .qolip
                .rename_block(&current.name, new_block, &warehouse)
                .await
                .map_err(|_| qolip_error(QolipError::StoreFailed))?;

            Ok(Json(serde_json::json!({
                "ok": true,
                "block": saved,
            })))
        }
        Method::DELETE => {
            let input: QolipBlockUpsert =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let is_admin = state
                .admin
                .principal_has_capability(&principal, Capability::AdminAccess)
                .await;
            let current = managed_qolip_block(&state, &principal, &input.block, is_admin).await?;
            ensure_qolip_block_is_empty(&state, &principal, &current, is_admin).await?;
            state
                .warehouses
                .delete_warehouse(WarehouseDeleteRequest {
                    warehouse: current.name,
                    delete_products: false,
                })
                .await
                .map_err(qolip_block_delete_error)?;
            Ok(Json(serde_json::json!({"ok": true})))
        }
        _ => Err(method_not_allowed()),
    }
}

async fn managed_qolip_block(
    state: &AppState,
    principal: &crate::core::auth::models::Principal,
    block: &str,
    is_admin: bool,
) -> Result<QolipBlock, (StatusCode, Json<QolipErrorResponse>)> {
    let block = block.trim();
    if block.is_empty() {
        return Err(bad_request("block_required"));
    }
    state
        .qolip
        .blocks_for_principal(principal, is_admin)
        .await
        .map_err(qolip_error)?
        .into_iter()
        .find(|item| item.name.trim().eq_ignore_ascii_case(block))
        .ok_or_else(forbidden)
}

async fn ensure_qolip_block_is_empty(
    state: &AppState,
    principal: &crate::core::auth::models::Principal,
    block: &QolipBlock,
    is_admin: bool,
) -> Result<(), (StatusCode, Json<QolipErrorResponse>)> {
    let locations = state
        .qolip
        .locations(&block.name)
        .await
        .map_err(qolip_error)?;
    if !locations.is_empty() {
        return Err(conflict("block_in_use"));
    }
    let open_checkouts = state
        .qolip
        .checkouts(principal, is_admin, Some(&block.name), "open", 1)
        .await
        .map_err(qolip_error)?;
    if open_checkouts.is_empty() {
        Ok(())
    } else {
        Err(conflict("block_in_use"))
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
    if method != Method::POST && method != Method::DELETE {
        return Err(method_not_allowed());
    }
    let principal = authenticated_principal(&state, &headers).await?;
    ensure_qolip_access(&state, &principal).await?;
    match method {
        Method::POST => {
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
                    "is_in_use": false,
                },
            })))
        }
        Method::DELETE => {
            let input: QolipProductSpecDelete =
                serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
            let deleted_count = state
                .qolip
                .delete_product_specs(input.qolip_codes)
                .await
                .map_err(qolip_error)?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "deleted_count": deleted_count,
            })))
        }
        _ => Err(method_not_allowed()),
    }
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
                } else if assigned.is_empty()
                    && !state
                        .admin
                        .principal_has_capability(&principal, Capability::AdminAccess)
                        .await
                {
                    return Err(forbidden());
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
    let mut input: QolipLocationMove =
        serde_json::from_slice(&body).map_err(|_| bad_request("invalid_json"))?;
    let location = state
        .qolip
        .location_by_id(&input.location_id)
        .await
        .map_err(qolip_error)?
        .ok_or_else(|| bad_request("location_not_found"))?;
    let _ = accessible_qolip_block(&state, &principal, &location.block).await?;
    let requested_block = input.block.trim();
    if requested_block.is_empty() || requested_block.eq_ignore_ascii_case(location.block.trim()) {
        input.block = location.block.clone();
        input.warehouse = location.warehouse.clone();
    } else {
        let target = match accessible_qolip_block(&state, &principal, requested_block).await? {
            Some(block) => block,
            None => state
                .qolip
                .blocks_for_principal(&principal, true)
                .await
                .map_err(qolip_error)?
                .into_iter()
                .find(|block| block.name.trim().eq_ignore_ascii_case(requested_block))
                .ok_or_else(|| bad_request("block_not_found"))?,
        };
        input.block = target.name;
        input.warehouse = target.warehouse;
    }
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

include!("qolip_print_scan.rs");
