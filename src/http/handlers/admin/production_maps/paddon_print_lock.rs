use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PrintConfirmationRequest {
    code: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NextPaddonRequest {
    code: String,
    apparatus: String,
}

pub(super) async fn can_unlock_snapshot(
    state: &AppState,
    principal: &Principal,
    snapshot: &crate::core::production_map::PaddonSnapshot,
) -> Result<bool, AdminError> {
    if snapshot.paddon.locked_at_unix.is_none() {
        return Ok(false);
    }
    for capability in [
        Capability::AdminAccess,
        Capability::ProductionMapManage,
        Capability::ApparatusQueueManage,
    ] {
        if state
            .admin
            .principal_has_capability(principal, capability)
            .await
        {
            return state
                .production_maps
                .can_unlock_paddon(&snapshot.paddon.code, &queue_action_actor(principal))
                .await
                .map_err(production_map_error);
        }
    }
    Ok(false)
}

pub async fn production_map_paddon_unlock(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    let input: PrintConfirmationRequest = parse_json(&body)?;
    super::paddons::require_paddon_visible(&state, &principal, &input.code).await?;
    state
        .production_maps
        .unlock_paddon(&input.code, &queue_action_actor(&principal))
        .await
        .map_err(production_map_error)?;
    let snapshot = state
        .production_maps
        .paddon_snapshot(&input.code)
        .await
        .map_err(production_map_error)?;
    let can_unlock = can_unlock_snapshot(&state, &principal, &snapshot).await?;
    Ok(json_response(serde_json::json!({
        "ok": true, "paddon": snapshot.paddon, "items": snapshot.items,
        "available_items": snapshot.available_items,
        "free_movement_enabled": snapshot.free_movement_enabled,
        "can_manage_items": snapshot.can_manage_items, "can_unlock": can_unlock,
    })))
}

pub(super) async fn assigned_cut_apparatuses(
    state: &AppState,
    principal: &Principal,
) -> Result<Vec<(String, String)>, AdminError> {
    if principal.role != PrincipalRole::Aparatchi {
        return Ok(Vec::new());
    }
    let assigned = state.admin.principal_assigned_apparatus(principal).await;
    let mut options = Vec::new();
    for id in assigned {
        let apparatus = super::queue_actions::resolve_queue_apparatus(state, &id).await?;
        if apparatus.is_rezka() {
            options.push((apparatus.id.to_string(), apparatus.display_name));
        }
    }
    options.sort();
    options.dedup();
    Ok(options)
}

pub async fn production_map_paddon_print_confirm(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal =
        authorize_any_capability(&state, &headers, &[Capability::ApparatusQueueManage]).await?;
    let options = assigned_cut_apparatuses(&state, &principal).await?;
    if options.is_empty() {
        return Err(forbidden());
    }
    let input: PrintConfirmationRequest = parse_json(&body)?;
    super::paddons::require_paddon_visible(&state, &principal, &input.code).await?;
    let result = state
        .production_maps
        .confirm_paddon_print(&input.code, &queue_action_actor(&principal))
        .await
        .map_err(production_map_error)?;
    let selected: Vec<_> = options
        .iter()
        .filter(|(id, _)| result.apparatuses.contains(id))
        .collect();
    let apparatus = if selected.len() == 1 {
        Some(selected[0].0.clone())
    } else if options.len() == 1 {
        Some(options[0].0.clone())
    } else {
        None
    };
    Ok(json_response(serde_json::json!({
        "ok": true, "paddon": result.paddon, "newly_locked": result.newly_locked,
        "apparatus": apparatus,
        "apparatus_options": options.into_iter().map(|(id, name)| serde_json::json!({"id":id,"name":name})).collect::<Vec<_>>(),
    })))
}

pub async fn production_map_paddon_next(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal =
        authorize_any_capability(&state, &headers, &[Capability::ApparatusQueueManage]).await?;
    let input: NextPaddonRequest = parse_json(&body)?;
    super::paddons::require_paddon_visible(&state, &principal, &input.code).await?;
    let options = assigned_cut_apparatuses(&state, &principal).await?;
    if !options.iter().any(|(id, _)| id == input.apparatus.trim()) {
        return Err(forbidden());
    }
    let paddon = state
        .production_maps
        .create_active_paddon_successor(
            &input.code,
            &input.apparatus,
            &queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(
        serde_json::json!({"ok":true, "apparatus":input.apparatus.trim(), "code":paddon.code, "paddon":paddon}),
    ))
}
