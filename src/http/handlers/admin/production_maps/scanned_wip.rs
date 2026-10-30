use super::*;
use crate::core::production_map::ScannedWipReturn;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScannedWipQuery {
    apparatus: String,
    order_id: String,
    stage_node_id: String,
}

pub async fn production_map_scanned_wips(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ScannedWipQuery>,
) -> Result<Response, AdminError> {
    let principal = authorize_capability(&state, &headers, Capability::ApparatusQueueRead).await?;
    if principal.role != PrincipalRole::Aparatchi {
        return Err(forbidden());
    }
    let assigned = state.admin.principal_assigned_apparatus(&principal).await;
    let inputs = state
        .production_maps
        .scanned_wip_inputs(
            query.apparatus.trim(),
            query.order_id.trim(),
            query.stage_node_id.trim(),
            &assigned,
            queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    let mut response = json_response(serde_json::json!({"items":inputs}));
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub async fn production_map_return_scanned_wip(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal =
        authorize_capability(&state, &headers, Capability::ApparatusQueueManage).await?;
    if principal.role != PrincipalRole::Aparatchi {
        return Err(forbidden());
    }
    let target: ScannedWipReturn = parse_json(&body)?;
    let assigned = state.admin.principal_assigned_apparatus(&principal).await;
    state
        .production_maps
        .return_scanned_wip(&target, &assigned, queue_action_actor(&principal))
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({"ok":true})))
}
