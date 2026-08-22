use super::*;
use crate::core::authz::assigned_apparatus_contains;

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
            require_role_grant_scope(&state, &principal, &input.capability_codes, &[], &[])
                .await?;
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
            let role_definitions = state
                .admin
                .role_definitions()
                .await
                .map_err(|_| server_error("admin roles fetch failed"))?;
            if let Some(role) = role_definitions
                .iter()
                .find(|role| role.id.eq_ignore_ascii_case(input.role_id.trim()))
            {
                require_role_grant_scope(
                    &state,
                    &principal,
                    &role.capability_codes,
                    &input.assigned_apparatus,
                    &input.assigned_item_groups,
                )
                .await?;
            }
            match state.admin.upsert_role_assignment(input).await {
                Ok(assignment) => {
                    revoke_role_assignment_sessions(&state, &assignment).await?;
                    Ok(json_response(assignment))
                }
                Err(AdminPortError::InvalidInput(message)) => Err(bad_request(message)),
                Err(_) => Err(server_error("admin role assignment save failed")),
            }
        }
        _ => Err(method_not_allowed()),
    }
}

async fn require_role_grant_scope(
    state: &AppState,
    principal: &Principal,
    capability_codes: &[String],
    assigned_apparatus: &[String],
    assigned_item_groups: &[String],
) -> Result<(), AdminError> {
    if state
        .admin
        .principal_has_capability(principal, Capability::AdminAccess)
        .await
    {
        return Ok(());
    }

    let principal_capabilities = state.admin.principal_capability_codes(principal).await;
    if capability_codes.iter().any(|requested| {
        let requested = requested.trim();
        !requested.is_empty()
            && !principal_capabilities
                .iter()
                .any(|allowed| allowed.trim().eq_ignore_ascii_case(requested))
    }) {
        return Err(forbidden());
    }

    let requested_apparatus = assigned_apparatus
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if !requested_apparatus.is_empty() {
        let principal_apparatus = state.admin.principal_assigned_apparatus(principal).await;
        if requested_apparatus
            .iter()
            .any(|requested| !assigned_apparatus_contains(requested, &principal_apparatus))
        {
            return Err(forbidden());
        }
    }

    let requested_item_groups = assigned_item_groups
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !requested_item_groups.is_empty() {
        let principal_item_groups = state
            .admin
            .principal_assigned_item_group_scope(principal)
            .await
            .map_err(|_| server_error("role assignment scope fetch failed"))?;
        let requested_item_group_scope = state
            .admin
            .item_group_scope(requested_item_groups)
            .await
            .map_err(|_| server_error("role assignment scope fetch failed"))?;
        if requested_item_group_scope.iter().any(|requested| {
            !principal_item_groups
                .iter()
                .any(|allowed| allowed.trim().eq_ignore_ascii_case(requested.trim()))
        }) {
            return Err(forbidden());
        }
    }

    Ok(())
}

async fn revoke_role_assignment_sessions(
    state: &AppState,
    assignment: &crate::core::authz::RoleAssignment,
) -> Result<(), AdminError> {
    let mut roles = vec![assignment.principal_role.clone()];
    match assignment.principal_role {
        PrincipalRole::Aparatchi => roles.push(PrincipalRole::Customer),
        PrincipalRole::Customer => roles.push(PrincipalRole::Aparatchi),
        _ => {}
    }
    for role in roles {
        state
            .sessions
            .delete_for_principal(&role, &assignment.principal_ref)
            .await
            .map_err(|_| server_error("role assignment session revoke failed"))?;
    }
    Ok(())
}
