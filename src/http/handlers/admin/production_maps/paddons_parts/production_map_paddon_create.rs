pub async fn production_map_paddon_create(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
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
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: PaddonCreateRequest = parse_json(&body)?;
    if principal.role.is_apparatus_operator()
        && input
            .apparatus
            .as_deref()
            .is_none_or(|id| id.trim().is_empty())
    {
        return Err(bad_request("apparatus is required"));
    }
    let apparatus = match input.apparatus.as_deref() {
        Some(id) => Some(resolve_paddon_apparatus(&state, &principal, id).await?),
        None => None,
    };
    let paddon = state
        .production_maps
        .create_paddon_for_apparatus(
            apparatus.as_deref(),
            &input.location,
            &input.note,
            &queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": paddon,
    })))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PaddonDeleteRequest {
    code: String,
}

pub async fn production_map_paddon_delete(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
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
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: PaddonDeleteRequest = parse_json(&body)?;
    require_paddon_visible(&state, &principal, &input.code).await?;
    state
        .production_maps
        .delete_paddon(&input.code)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({"ok": true})))
}

pub async fn production_map_paddon_item_add(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
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
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: PaddonItemRequest = parse_json(&body)?;
    require_paddon_visible(&state, &principal, &input.code).await?;
    let progress_batch_id = resolve_progress_batch_id(&state, &input).await?;
    let snapshot = add_items_for_principal(&state, &principal, &input.code, &[progress_batch_id]).await?;
    let can_unlock =
        super::paddon_print_lock::can_unlock_snapshot(&state, &principal, &snapshot).await?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "available_items": snapshot.available_items,
        "free_movement_enabled": snapshot.free_movement_enabled,
        "can_manage_items": snapshot.can_manage_items,
        "can_unlock": can_unlock,
    })))
}

pub async fn production_map_paddon_items_add(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
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
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: PaddonItemsRequest = parse_json(&body)?;
    require_paddon_visible(&state, &principal, &input.code).await?;
    let snapshot = add_items_for_principal(&state, &principal, &input.code, &input.progress_batch_ids).await?;
    let can_unlock =
        super::paddon_print_lock::can_unlock_snapshot(&state, &principal, &snapshot).await?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "available_items": snapshot.available_items,
        "free_movement_enabled": snapshot.free_movement_enabled,
        "can_manage_items": snapshot.can_manage_items,
        "can_unlock": can_unlock,
    })))
}

pub async fn production_map_paddon_item_remove(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
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
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: PaddonItemRequest = parse_json(&body)?;
    require_paddon_visible(&state, &principal, &input.code).await?;
    let progress_batch_id = input.progress_batch_id.trim();
    if progress_batch_id.is_empty() {
        return Err(bad_request("progress_batch_id_required"));
    }
    let snapshot = state
        .production_maps
        .remove_paddon_item(
            &input.code,
            progress_batch_id,
            &queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    let snapshot = refresh_mutation_candidates(&state, &principal, snapshot).await;
    let can_unlock =
        super::paddon_print_lock::can_unlock_snapshot(&state, &principal, &snapshot).await?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "available_items": snapshot.available_items,
        "free_movement_enabled": snapshot.free_movement_enabled,
        "can_manage_items": snapshot.can_manage_items,
        "can_unlock": can_unlock,
    })))
}

pub async fn production_map_paddon_items_remove(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
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
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: PaddonItemsRequest = parse_json(&body)?;
    require_paddon_visible(&state, &principal, &input.code).await?;
    let snapshot = state
        .production_maps
        .remove_paddon_items(
            &input.code,
            &input.progress_batch_ids,
            &queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    let snapshot = refresh_mutation_candidates(&state, &principal, snapshot).await;
    let can_unlock =
        super::paddon_print_lock::can_unlock_snapshot(&state, &principal, &snapshot).await?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "available_items": snapshot.available_items,
        "free_movement_enabled": snapshot.free_movement_enabled,
        "can_manage_items": snapshot.can_manage_items,
        "can_unlock": can_unlock,
    })))
}

async fn resolve_progress_batch_id(
    state: &AppState,
    input: &PaddonItemRequest,
) -> Result<String, AdminError> {
    let progress_batch_id = input.progress_batch_id.trim();
    if !progress_batch_id.is_empty() {
        return Ok(progress_batch_id.to_string());
    }
    let qr_payload = input.qr_payload.trim();
    if qr_payload.is_empty() {
        return Err(bad_request("progress_qr_required"));
    }
    let batch = state
        .production_maps
        .progress_batch_for_qr("", qr_payload)
        .await
        .map_err(production_map_error)?;
    Ok(batch.batch_id)
}
