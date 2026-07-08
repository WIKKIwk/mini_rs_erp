use super::apparatus::{apparatus_warehouse, is_apparat_parent};
use super::*;
use crate::core::admin::models::AdminWarehouse;
use crate::core::warehouses::{WarehouseAssignment, WarehouseSummary};

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
            Capability::RawMaterialAssign,
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
    let material_scope = material_warehouse_scope(&state, &principal).await?;
    let fetch_limit = if material_scope.is_some() { 500 } else { limit };
    let mut warehouses = state
        .admin
        .warehouses(
            query.q.as_deref().unwrap_or(""),
            query.parent.as_deref().unwrap_or(""),
            fetch_limit,
        )
        .await
        .map_err(|_| server_error("admin warehouses fetch failed"))?;
    let mini_warehouses = state
        .warehouses
        .warehouses(
            query.q.as_deref().unwrap_or(""),
            query.parent.as_deref().unwrap_or(""),
            fetch_limit,
        )
        .await
        .map_err(warehouse_error)?;
    warehouses =
        crate::core::warehouses::merge_admin_warehouses(warehouses, mini_warehouses, fetch_limit);
    if is_apparat_parent(query.parent.as_deref().unwrap_or("")) {
        let remaining = fetch_limit.saturating_sub(warehouses.len()).max(1);
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
            if warehouses.len() >= fetch_limit {
                break;
            }
        }
        warehouses.sort_by(|left, right| {
            left.warehouse
                .to_lowercase()
                .cmp(&right.warehouse.to_lowercase())
        });
    }
    if let Some(scope) = material_scope.as_ref() {
        warehouses = scoped_warehouses(warehouses, scope);
    }
    warehouses.truncate(limit);
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
            Capability::RawMaterialAssign,
        ],
    )
    .await?;
    if principal.role == PrincipalRole::MaterialTaminotchi {
        require_capability(&state, &principal, Capability::RawMaterialAssign).await?;
    } else {
        require_capability(&state, &principal, Capability::CatalogItemRead).await?;
    }
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let limit = optional_search_limit(query.limit.as_deref(), 30, 500);
    let material_scope = material_warehouse_scope(&state, &principal).await?;
    let fetch_limit = if material_scope.is_some() { 500 } else { limit };
    let mut summaries = state
        .warehouses
        .warehouse_summaries(query.q.as_deref().unwrap_or(""), fetch_limit)
        .await
        .map_err(warehouse_error)?;
    if let Some(scope) = material_scope.as_ref() {
        summaries = scoped_summaries(summaries, scope);
    }
    summaries.truncate(limit);
    Ok(json_response(summaries))
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
        &[
            Capability::AdminAccess,
            Capability::CatalogItemRead,
            Capability::RawMaterialAssign,
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::POST) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            if principal.role == PrincipalRole::MaterialTaminotchi {
                require_capability(&state, &principal, Capability::RawMaterialAssign).await?;
            } else {
                require_capability(&state, &principal, Capability::CatalogItemRead).await?;
            }
            let mut assignments = state
                .warehouses
                .warehouse_assignments(query.warehouse.as_deref().unwrap_or(""))
                .await
                .map_err(warehouse_error)?;
            if principal.role == PrincipalRole::MaterialTaminotchi {
                assignments = scoped_assignments_for_principal(assignments, &principal);
            }
            Ok(json_response(assignments))
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

async fn material_warehouse_scope(
    state: &AppState,
    principal: &Principal,
) -> Result<Option<std::collections::BTreeSet<String>>, AdminError> {
    if principal.role != PrincipalRole::MaterialTaminotchi {
        return Ok(None);
    }
    let assigned = state
        .warehouses
        .assigned_warehouse_names(principal)
        .await
        .map_err(warehouse_error)?;
    Ok(Some(
        assigned
            .into_iter()
            .map(|warehouse| warehouse.trim().to_lowercase())
            .filter(|warehouse| !warehouse.is_empty())
            .collect(),
    ))
}

fn scoped_warehouses(
    warehouses: Vec<AdminWarehouse>,
    scope: &std::collections::BTreeSet<String>,
) -> Vec<AdminWarehouse> {
    warehouses
        .into_iter()
        .filter(|warehouse| scope.contains(&warehouse.warehouse.trim().to_lowercase()))
        .collect()
}

fn scoped_summaries(
    summaries: Vec<WarehouseSummary>,
    scope: &std::collections::BTreeSet<String>,
) -> Vec<WarehouseSummary> {
    summaries
        .into_iter()
        .filter(|summary| scope.contains(&summary.warehouse.trim().to_lowercase()))
        .collect()
}

fn scoped_assignments_for_principal(
    assignments: Vec<WarehouseAssignment>,
    principal: &Principal,
) -> Vec<WarehouseAssignment> {
    assignments
        .into_iter()
        .filter(|assignment| {
            assignment.principal_role == principal.role
                && assignment
                    .principal_ref
                    .trim()
                    .eq_ignore_ascii_case(principal.ref_.trim())
        })
        .collect()
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
