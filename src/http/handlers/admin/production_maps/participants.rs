use super::*;

#[derive(Deserialize)]
pub struct StageParticipantsQuery {
    order_id: String,
    stage_node_id: String,
    apparatus: String,
}

pub async fn production_map_stage_participants(
    State(state): State<AppState>,
    Query(query): Query<StageParticipantsQuery>,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    if query.order_id.trim().is_empty()
        || query.stage_node_id.trim().is_empty()
        || !queue_state::is_canonical_apparatus_id(query.apparatus.trim())
    {
        return Err(bad_request("invalid_stage_participants_scope"));
    }
    let participants = state
        .production_maps
        .stage_participants(&query.order_id, &query.stage_node_id, &query.apparatus)
        .await
        .map_err(production_map_error)?
        .ok_or_else(|| not_found("order_stage_not_found"))?;
    let mut response = json_response(serde_json::json!({ "participants": participants }));
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}
