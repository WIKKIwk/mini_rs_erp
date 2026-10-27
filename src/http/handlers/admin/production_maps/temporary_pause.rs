use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TemporaryPauseRequest {
    apparatus: String,
    order_id: String,
    session_id: String,
    token: String,
}

pub async fn production_map_temporary_pause(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal =
        authorize_capability(&state, &headers, Capability::ApparatusQueueManage).await?;
    if principal.role != PrincipalRole::Aparatchi {
        return Err(forbidden());
    }
    let request: TemporaryPauseRequest = parse_json(&body)?;
    let assigned = state.admin.principal_assigned_apparatus(&principal).await;
    state
        .production_maps
        .temporarily_pause_work(
            request.apparatus.trim(),
            request.order_id.trim(),
            request.session_id.trim(),
            request.token.trim(),
            &assigned,
            queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({"ok":true})))
}
