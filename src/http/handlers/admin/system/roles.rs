use super::*;

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
