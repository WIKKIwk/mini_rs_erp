use super::apparatus::{apparatus_warehouse, is_apparat_parent};
use super::*;

pub async fn warehouses(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ItemQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::CatalogItemRead,
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    if method == Method::POST {
        require_capability(&state, &principal, Capability::ProductionMapManage).await?;
        let input: WarehouseUpsert = parse_json(&body)?;
        let saved = state
            .warehouses
            .upsert_warehouse(input)
            .await
            .map_err(warehouse_error)?;
        state
            .warehouse_events
            .notify_updated(&saved.warehouse, "warehouse");
        return Ok(json_response(saved));
    }
    let limit = optional_search_limit(query.limit.as_deref(), 30, 500);
    let mut warehouses = state
        .admin
        .warehouses(
            query.q.as_deref().unwrap_or(""),
            query.parent.as_deref().unwrap_or(""),
            limit,
        )
        .await
        .map_err(|_| server_error("admin warehouses fetch failed"))?;
    let mini_warehouses = state
        .warehouses
        .warehouses(
            query.q.as_deref().unwrap_or(""),
            query.parent.as_deref().unwrap_or(""),
            limit,
        )
        .await
        .map_err(warehouse_error)?;
    warehouses =
        crate::core::warehouses::merge_admin_warehouses(warehouses, mini_warehouses, limit);
    if is_apparat_parent(query.parent.as_deref().unwrap_or("")) {
        let remaining = limit.saturating_sub(warehouses.len()).max(1);
        let mut seen = warehouses
            .iter()
            .map(|item| item.warehouse.to_lowercase())
            .collect::<std::collections::BTreeSet<_>>();
        for name in state
            .apparatus_groups
            .apparatus(query.q.as_deref().unwrap_or(""), remaining)
            .await
            .map_err(apparatus_group_error)?
        {
            if seen.insert(name.to_lowercase()) {
                warehouses.push(apparatus_warehouse(name));
            }
            if warehouses.len() >= limit {
                break;
            }
        }
        warehouses.sort_by(|left, right| {
            left.warehouse
                .to_lowercase()
                .cmp(&right.warehouse.to_lowercase())
        });
    }
    Ok(json_response(warehouses))
}

pub async fn warehouse_summaries(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ItemQuery>,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::CatalogItemRead,
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    require_capability(&state, &principal, Capability::CatalogItemRead).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let limit = optional_search_limit(query.limit.as_deref(), 30, 500);
    state
        .warehouses
        .warehouse_summaries(query.q.as_deref().unwrap_or(""), limit)
        .await
        .map(json_response)
        .map_err(warehouse_error)
}

pub async fn warehouse_assignments(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ItemQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::CatalogItemRead],
    )
    .await?;
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            require_capability(&state, &principal, Capability::CatalogItemRead).await?;
            state
                .warehouses
                .warehouse_assignments(query.warehouse.as_deref().unwrap_or(""))
                .await
                .map(json_response)
                .map_err(warehouse_error)
        }
        Method::POST => {
            require_capability(&state, &principal, Capability::AdminAccess).await?;
            let input: WarehouseAssignmentUpsert = parse_json(&body)?;
            let saved = state
                .warehouses
                .assign_warehouse(input)
                .await
                .map_err(warehouse_error)?;
            state
                .warehouse_events
                .notify_updated(&saved.warehouse, "warehouse_assignment");
            Ok(json_response(saved))
        }
        _ => Err(method_not_allowed()),
    }
}

fn warehouse_error(error: WarehouseError) -> AdminError {
    match error {
        WarehouseError::MissingWarehouse => bad_request("warehouse is required"),
        WarehouseError::MissingPrincipalRef => bad_request("principal ref is required"),
        WarehouseError::StoreFailed => server_error("warehouse store failed"),
    }
}

fn apparatus_group_error(error: ApparatusGroupError) -> AdminError {
    match error {
        ApparatusGroupError::MissingName => bad_request("group name is required"),
        ApparatusGroupError::MissingApparatus => bad_request("apparatus is required"),
        ApparatusGroupError::StoreFailed => server_error("apparatus group store failed"),
    }
}
