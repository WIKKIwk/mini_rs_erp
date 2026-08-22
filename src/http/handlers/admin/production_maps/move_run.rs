use super::*;

pub async fn production_map_move_batch(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: ProductionMapBatchMoveRequest = parse_json(&body)?;
    for map_id in &input.map_ids {
        reject_training_order_id_for_production(map_id)?;
    }
    match state.production_maps.move_apparatus_batch(input).await {
        Ok(saved) => Ok(json_response(serde_json::json!({
            "ok": true,
            "saved": saved,
        }))),
        Err(error) => Err(production_map_error(error)),
    }
}

/// Transfers a paused, already-started order to a compatible replacement
/// apparatus together with its queue/session/progress/material state.
pub async fn production_map_apparatus_transfer(
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
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: ProductionMapApparatusTransferRequest = parse_json(&body)?;
    reject_training_order_id_for_production(&input.order_id)?;
    if input.order_id.trim().is_empty()
        || input.from_apparatus.trim().is_empty()
        || input.to_apparatus.trim().is_empty()
    {
        return Err(bad_request("apparatus transfer input is incomplete"));
    }
    match state
        .production_maps
        .transfer_apparatus_order(input, queue_action_actor(&principal))
        .await
    {
        Ok(result) => Ok(json_response(serde_json::json!({
            "ok": true,
            "transfer_id": result.transfer.transfer_id,
            "saved": result.saved,
            "order_status": result.order_status,
            "queue_state": "paused",
            "session_id": result.transfer.session_id,
            "progress_batch_id": result.transfer.progress_batch_id,
            "material_barcodes": result.transfer.material_barcodes,
        }))),
        Err(error) => Err(production_map_error(error)),
    }
}

/// Moves an order between apparatus. Pechat compatibility is validated on the
/// server; the client only renders the outcome.
pub async fn production_map_move(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: ProductionMapMoveRequest = parse_json(&body)?;
    reject_training_order_id_for_production(&input.map_id)?;
    match state.production_maps.move_apparatus(input).await {
        Ok(saved) => Ok(json_response(serde_json::json!({
            "ok": true,
            "saved": saved,
        }))),
        Err(error) => Err(production_map_error(error)),
    }
}

pub async fn production_map_run(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: ProductionMapRunRequest = parse_json(&body)?;
    reject_training_order_id_for_production(&input.map_id)?;
    match state.production_maps.run_map(input).await {
        Ok(result) => Ok(json_response(result)),
        Err(error) => Err(bad_request(error.to_string())),
    }
}
