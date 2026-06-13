use super::*;

pub async fn apparatus_groups(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if !matches!(method, Method::GET | Method::PUT) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            require_capability(&state, &principal, Capability::ProductionMapManage).await?;
            state
                .apparatus_groups
                .groups()
                .await
                .map(json_response)
                .map_err(apparatus_group_error)
        }
        Method::PUT => {
            require_capability(&state, &principal, Capability::ProductionMapManage).await?;
            let input: ApparatusGroupUpsert = parse_json(&body)?;
            state
                .apparatus_groups
                .upsert_group(input)
                .await
                .map(json_response)
                .map_err(apparatus_group_error)
        }
        _ => Err(method_not_allowed()),
    }
}

fn apparatus_group_error(error: ApparatusGroupError) -> AdminError {
    match error {
        ApparatusGroupError::MissingName => bad_request("group name is required"),
        ApparatusGroupError::MissingApparatus => bad_request("apparatus is required"),
        ApparatusGroupError::StoreFailed => server_error("apparatus group store failed"),
    }
}

pub async fn items_bulk_move_group(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<AdminItemGroupBulkMoveResult>, AdminError> {
    authorize_capability(&state, &headers, Capability::CatalogItemBulkMove).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: AdminBulkMoveItemsRequest = parse_json(&body)?;
    match state
        .admin
        .move_items_to_group(input.item_codes, &input.item_group)
        .await
    {
        Ok(result) => Ok(Json(result)),
        Err(AdminPortError::InvalidInput(message)) => Err(bad_request(message)),
        Err(_) => Err(server_error("admin item bulk move failed")),
    }
}

pub async fn warehouses(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<ItemQuery>,
) -> Result<Response, AdminError> {
    authorize_any_capability(
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
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let limit = optional_search_limit(query.limit.as_deref(), 30, 500);
    state
        .admin
        .warehouses(
            query.q.as_deref().unwrap_or(""),
            query.parent.as_deref().unwrap_or(""),
            limit,
        )
        .await
        .map(json_response)
        .map_err(|_| server_error("admin warehouses fetch failed"))
}

pub async fn werka_code_regenerate(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Json<AdminSettings>, AdminError> {
    authorize_capability(&state, &headers, Capability::WerkaCodeManage).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    match state.admin.regenerate_werka_code().await {
        Ok(settings) => Ok(Json(settings)),
        Err(AdminPortError::CodeRegenCooldown) => {
            Err(too_many_requests("code regenerate cooldown"))
        }
        Err(_) => Err(server_error("werka code regenerate failed")),
    }
}

pub async fn capabilities(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::RoleCapabilityRead).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    Ok(json_response(capability_catalog_entries()))
}

pub async fn roles(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::RoleCapabilityRead,
            Capability::RoleCapabilityManage,
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::PUT) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            require_capability(&state, &principal, Capability::RoleCapabilityRead).await?;
            state
                .admin
                .role_definitions()
                .await
                .map(json_response)
                .map_err(|_| server_error("admin roles fetch failed"))
        }
        Method::PUT => {
            require_capability(&state, &principal, Capability::RoleCapabilityManage).await?;
            let input: RoleDefinitionUpsert = parse_json(&body)?;
            match state.admin.upsert_role_definition(input).await {
                Ok(role) => Ok(json_response(role)),
                Err(AdminPortError::InvalidInput(message)) => Err(bad_request(message)),
                Err(_) => Err(server_error("admin role save failed")),
            }
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn role_assignments(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::RoleCapabilityRead,
            Capability::RoleCapabilityManage,
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::PUT) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            require_capability(&state, &principal, Capability::RoleCapabilityRead).await?;
            state
                .admin
                .role_assignments()
                .await
                .map(json_response)
                .map_err(|_| server_error("admin role assignments fetch failed"))
        }
        Method::PUT => {
            require_capability(&state, &principal, Capability::RoleCapabilityManage).await?;
            let input: RoleAssignmentUpsert = parse_json(&body)?;
            match state.admin.upsert_role_assignment(input).await {
                Ok(assignment) => Ok(json_response(assignment)),
                Err(AdminPortError::InvalidInput(message)) => Err(bad_request(message)),
                Err(_) => Err(server_error("admin role assignment save failed")),
            }
        }
        _ => Err(method_not_allowed()),
    }
}

pub(super) async fn authorize_capability(
    state: &AppState,
    headers: &HeaderMap,
    capability: Capability,
) -> Result<Principal, AdminError> {
    let principal = authenticated_principal(state, headers).await?;
    require_capability(state, &principal, capability).await?;
    Ok(principal)
}

pub(super) async fn authorize_any_capability(
    state: &AppState,
    headers: &HeaderMap,
    capabilities: &[Capability],
) -> Result<Principal, AdminError> {
    let principal = authenticated_principal(state, headers).await?;
    for capability in capabilities {
        if state
            .admin
            .principal_has_capability(&principal, *capability)
            .await
        {
            return Ok(principal);
        }
    }
    Err(forbidden())
}

pub(super) async fn require_capability(
    state: &AppState,
    principal: &Principal,
    capability: Capability,
) -> Result<(), AdminError> {
    if state
        .admin
        .principal_has_capability(principal, capability)
        .await
    {
        Ok(())
    } else {
        Err(forbidden())
    }
}

async fn authenticated_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, AdminError> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}
