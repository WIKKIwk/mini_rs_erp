use super::*;
use crate::core::inventory_movements::{
    InventoryActor, InventoryAssetQuery, InventoryMovementError, InventoryRelocationCreate,
    InventoryTransferAction, InventoryTransferActionKind, InventoryTransferCreate,
    InventoryTransferQuery,
};

pub async fn inventory_locations(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let _ = inventory_actor(&state, &headers).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    state
        .inventory_movements
        .locations()
        .await
        .map(json_response)
        .map_err(inventory_error)
}

pub async fn inventory_assets(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<InventoryAssetQuery>,
) -> Result<Response, AdminError> {
    let actor = inventory_actor(&state, &headers).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    state
        .inventory_movements
        .assets(&actor, query)
        .await
        .map(json_response)
        .map_err(inventory_error)
}

pub async fn inventory_relocations(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let actor = inventory_actor(&state, &headers).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: InventoryRelocationCreate = parse_json(&body)?;
    let asset = state
        .inventory_movements
        .relocate(&actor, input)
        .await
        .map_err(inventory_error)?;
    state
        .warehouse_events
        .notify_updated(&asset.custody_warehouse, "inventory_relocated");
    Ok(json_response(asset))
}

pub async fn inventory_transfers(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<InventoryTransferQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let actor = inventory_actor(&state, &headers).await?;
    match method {
        Method::GET => state
            .inventory_movements
            .transfers(&actor, query)
            .await
            .map(json_response)
            .map_err(inventory_error),
        Method::POST => {
            let input: InventoryTransferCreate = parse_json(&body)?;
            let transfer = state
                .inventory_movements
                .create_transfer(&actor, input)
                .await
                .map_err(inventory_error)?;
            state
                .warehouse_events
                .notify_updated(&transfer.source_warehouse, "inventory_transfer_requested");
            state.warehouse_events.notify_updated(
                &transfer.destination_warehouse,
                "inventory_transfer_requested",
            );
            Ok(json_response(transfer))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn inventory_transfer_action(
    State(state): State<AppState>,
    Path((id, action)): Path<(String, String)>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let actor = inventory_actor(&state, &headers).await?;
    if !matches!(method, Method::POST | Method::PUT) {
        return Err(method_not_allowed());
    }
    let action = InventoryTransferActionKind::parse(&action).map_err(inventory_error)?;
    let input: InventoryTransferAction = parse_json(&body)?;
    let transfer = state
        .inventory_movements
        .transfer_action(&actor, &id, action, input)
        .await
        .map_err(inventory_error)?;
    state
        .warehouse_events
        .notify_updated(&transfer.source_warehouse, "inventory_transfer_updated");
    state.warehouse_events.notify_updated(
        &transfer.destination_warehouse,
        "inventory_transfer_updated",
    );
    Ok(json_response(transfer))
}

async fn inventory_actor(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<InventoryActor, AdminError> {
    let principal =
        authorize_capability(state, headers, Capability::InventoryMovementManage).await?;
    let is_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    let assigned_warehouses = state
        .warehouses
        .assigned_warehouse_names(&principal)
        .await
        .map_err(|_| server_error("inventory warehouse scope failed"))?;
    Ok(InventoryActor::new(
        principal,
        is_admin,
        assigned_warehouses,
    ))
}

fn inventory_error(error: InventoryMovementError) -> AdminError {
    match error {
        InventoryMovementError::InvalidAssetKind => bad_request("inventory_asset_kind_invalid"),
        InventoryMovementError::MissingAssetRef => bad_request("inventory_asset_ref_required"),
        InventoryMovementError::AssetNotFound => not_found("inventory_asset_not_found"),
        InventoryMovementError::AssetUnavailable => conflict("inventory_asset_unavailable"),
        InventoryMovementError::AssetNotInSourceWarehouse => {
            conflict("inventory_asset_not_in_source_warehouse")
        }
        InventoryMovementError::InvalidLocation => bad_request("inventory_location_invalid"),
        InventoryMovementError::LocationNotFound => not_found("inventory_location_not_found"),
        InventoryMovementError::LocationInactive => conflict("inventory_location_inactive"),
        InventoryMovementError::CrossWarehouseRelocation => {
            conflict("inventory_cross_warehouse_requires_transfer")
        }
        InventoryMovementError::MissingWarehouse => bad_request("inventory_warehouse_required"),
        InventoryMovementError::WarehouseNotFound => not_found("inventory_warehouse_not_found"),
        InventoryMovementError::DestinationWarehouseUnassigned => {
            conflict("inventory_destination_warehouse_unassigned")
        }
        InventoryMovementError::SameWarehouse => bad_request("inventory_transfer_same_warehouse"),
        InventoryMovementError::MissingAssets => bad_request("inventory_transfer_assets_required"),
        InventoryMovementError::DuplicateAsset => bad_request("inventory_transfer_asset_duplicate"),
        InventoryMovementError::TransferNotFound => not_found("inventory_transfer_not_found"),
        InventoryMovementError::InvalidTransferStatus
        | InventoryMovementError::InvalidTransition => {
            conflict("inventory_transfer_transition_invalid")
        }
        InventoryMovementError::WarehouseForbidden => forbidden(),
        InventoryMovementError::MissingIdempotencyKey => {
            bad_request("inventory_idempotency_key_required")
        }
        InventoryMovementError::IdempotencyConflict => conflict("inventory_idempotency_conflict"),
        InventoryMovementError::StoreFailed => server_error("inventory movement store failed"),
    }
}
