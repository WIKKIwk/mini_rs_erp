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
    match state.production_maps.move_apparatus_batch(input).await {
        Ok(saved) => Ok(json_response(serde_json::json!({
            "ok": true,
            "saved": saved,
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
    match state.production_maps.run_map(input).await {
        Ok(result) => Ok(json_response(result)),
        Err(error) => Err(bad_request(error.to_string())),
    }
}
