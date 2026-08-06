use super::*;

#[derive(Default, serde::Deserialize)]
pub struct PaddonsQuery {
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Default, serde::Deserialize)]
struct PaddonCreateRequest {
    #[serde(default)]
    location: String,
    #[serde(default)]
    note: String,
}

#[derive(Default, serde::Deserialize)]
struct PaddonItemRequest {
    #[serde(default)]
    code: String,
    #[serde(default)]
    progress_batch_id: String,
    #[serde(default)]
    qr_payload: String,
}

pub async fn production_map_paddons(
    State(state): State<AppState>,
    Query(query): Query<PaddonsQuery>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueRead,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let paddons = state
        .production_maps
        .paddons(query.limit.unwrap_or(50))
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddons": paddons,
    })))
}

pub async fn production_map_paddon_detail(
    State(state): State<AppState>,
    Query(query): Query<PaddonCodeQuery>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueRead,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let snapshot = state
        .production_maps
        .paddon_snapshot(&query.code)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "available_items": snapshot.available_items,
    })))
}

#[derive(Default, serde::Deserialize)]
pub struct PaddonCodeQuery {
    #[serde(default)]
    code: String,
}

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
    let paddon = state
        .production_maps
        .create_paddon(
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
    let progress_batch_id = resolve_progress_batch_id(&state, &input).await?;
    let snapshot = state
        .production_maps
        .add_paddon_item(
            &input.code,
            &progress_batch_id,
            &queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "available_items": snapshot.available_items,
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
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "available_items": snapshot.available_items,
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
