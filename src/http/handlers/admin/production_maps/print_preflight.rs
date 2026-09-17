use super::queue_actions::resolve_queue_apparatus;
use super::*;

#[derive(serde::Deserialize)]
struct PrintPreflightRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    hold_id: String,
    #[serde(default)]
    idempotency_key: String,
    action: String,
}

pub async fn production_map_print_preflight(
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
    let input: PrintPreflightRequest = parse_json(&body)?;
    let apparatus = input.apparatus.trim();
    let order_id = input.order_id.trim();
    if apparatus.is_empty() || order_id.is_empty() {
        return Err(bad_request("apparatus and order_id are required"));
    }
    let apparatus = resolve_queue_apparatus(&state, apparatus).await?;
    if !apparatus.is_pechat() {
        return Err(bad_request("print_preflight_requires_print_apparatus"));
    }
    let is_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
    if !is_admin
        && !queue_state::apparatus_matches_assigned(&apparatus.id.to_string(), &assigned_apparatus)
    {
        return Err(bad_request("apparatus_not_assigned"));
    }
    let actor = queue_action_actor(&principal);
    let hold = match input.action.trim().to_ascii_lowercase().as_str() {
        "hold" => {
            if input.hold_id.trim().is_empty() || input.idempotency_key.trim().is_empty() {
                return Err(bad_request("hold_id and idempotency_key are required"));
            }
            state
                .production_maps
                .begin_print_preflight(
                    &apparatus.id.to_string(),
                    order_id,
                    &input.hold_id,
                    &input.idempotency_key,
                    actor,
                )
                .await
                .map_err(production_map_error)?
        }
        "start" | "passed" | "failed" | "cancel" => {
            if input.hold_id.trim().is_empty() {
                return Err(bad_request("hold_id is required"));
            }
            state
                .production_maps
                .advance_print_preflight(
                    &apparatus.id.to_string(),
                    order_id,
                    &input.hold_id,
                    input.action.trim(),
                    actor,
                )
                .await
                .map_err(production_map_error)?
        }
        _ => return Err(bad_request("print_preflight_action_invalid")),
    };
    Ok(json_response(serde_json::json!({
        "ok": true,
        "hold": hold,
    })))
}
