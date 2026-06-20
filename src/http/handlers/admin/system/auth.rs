use super::*;

pub(in crate::http::handlers::admin) async fn authorize_capability(
    state: &AppState,
    headers: &HeaderMap,
    capability: Capability,
) -> Result<Principal, AdminError> {
    let principal = authenticated_principal(state, headers).await?;
    require_capability(state, &principal, capability).await?;
    Ok(principal)
}

pub(in crate::http::handlers::admin) async fn authorize_any_capability(
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

pub(in crate::http::handlers::admin) async fn require_capability(
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
