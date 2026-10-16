use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RePauseRequest {
    apparatus: String,
    order_id: String,
    resume_id: String,
}

pub async fn production_map_re_pause(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal =
        authorize_capability(&state, &headers, Capability::ApparatusQueueManage).await?;
    if principal.role != PrincipalRole::Aparatchi {
        return Err(forbidden());
    }
    let request: RePauseRequest = parse_json(&body)?;
    let assigned = state.admin.principal_assigned_apparatus(&principal).await;
    state
        .production_maps
        .re_pause_resumed_work(
            request.apparatus.trim(),
            request.order_id.trim(),
            request.resume_id.trim(),
            &assigned,
            queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "rev": state.production_maps.snapshot_revision(),
        "epoch": state.production_maps.snapshot_epoch(),
    })))
}
