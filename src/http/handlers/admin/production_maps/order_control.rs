use super::*;

#[derive(serde::Deserialize)]
struct OrderControlRequest {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    action: String,
}

pub async fn production_map_order_control(
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
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    let input: OrderControlRequest = parse_json(&body)?;
    let order_id = input.order_id.trim();
    if order_id.is_empty() {
        return Err(bad_request("order_id is required"));
    }
    let actor = queue_action_actor(&principal);
    match input.action.trim() {
        "freeze" => {
            let control = state
                .production_maps
                .request_order_freeze(order_id, actor)
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "control": control,
            })))
        }
        "cancel_freeze" => {
            let control = state
                .production_maps
                .cancel_order_freeze_request(order_id, actor)
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "control": control,
            })))
        }
        "unfreeze" => {
            let control = state
                .production_maps
                .unfreeze_order(order_id, actor)
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "control": control,
            })))
        }
        "delete" => {
            let result = state
                .production_maps
                .delete_order(order_id)
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "result": result,
            })))
        }
        _ => Err(bad_request("order_control_action_invalid")),
    }
}
