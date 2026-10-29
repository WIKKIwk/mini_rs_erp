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
                Ok(role) => {
                    state.warehouse_events.invalidate_inventory();
                    Ok(json_response(role))
                }
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
            let input: RoleAssignmentUpsert = parse_json(&body)?;
            if principal.role == crate::core::auth::models::PrincipalRole::Brigader {
                require_capability(&state, &principal, Capability::BrigaderAccess).await?;
                let roles = state
                    .admin
                    .role_definitions()
                    .await
                    .map_err(|_| server_error("admin roles fetch failed"))?;
                let role = roles
                    .iter()
                    .find(|role| role.id == input.role_id.trim())
                    .ok_or_else(forbidden)?;
                let allowed = crate::core::authz::capability_codes_for_role(
                    crate::core::auth::models::PrincipalRole::Brigader,
                );
                super::super::system_users::require_brigader_identity_target(
                    &principal,
                    Some(&input.principal_ref),
                )?;
                // Existing staff forms assign trusted system roles (notably
                // apparatus-scoped Aparatchi) after creating the directory user.
                let staff_system_role = role.system && role.id != "admin";
                if input.principal_role == crate::core::auth::models::PrincipalRole::Admin
                    || (!staff_system_role
                        && role
                            .capability_codes
                            .iter()
                            .any(|code| !allowed.contains(code)))
                {
                    return Err(forbidden());
                }
            } else {
                require_capability(&state, &principal, Capability::RoleCapabilityManage).await?;
            }
            match state.admin.upsert_role_assignment(input).await {
                Ok(assignment) => {
                    state.warehouse_events.invalidate_inventory();
                    Ok(json_response(assignment))
                }
                Err(AdminPortError::InvalidInput(message)) => Err(bad_request(message)),
                Err(_) => Err(server_error("admin role assignment save failed")),
            }
        }
        _ => Err(method_not_allowed()),
    }
}
