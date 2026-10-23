use super::*;

#[derive(serde::Deserialize)]
struct OrderControlRequest {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    comment: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    stage_node_id: String,
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
    let scope = input.scope.trim();
    if !matches!(scope, "" | "order" | "stage") {
        return Err(bad_request("order_control_scope_invalid"));
    }
    if scope != "stage"
        && (!input.apparatus.trim().is_empty() || !input.stage_node_id.trim().is_empty())
    {
        return Err(bad_request("order_control_scope_invalid"));
    }
    if scope == "stage" {
        if input.apparatus.trim().is_empty() {
            return Err(bad_request("apparatus is required"));
        }
        let service = &state.production_maps;
        let control = match input.action.trim() {
            "freeze" => {
                service
                    .request_stage_freeze(order_id, &input.apparatus, &input.stage_node_id, actor)
                    .await
            }
            "cancel_freeze" => {
                service
                    .cancel_stage_freeze_request(
                        order_id,
                        &input.apparatus,
                        &input.stage_node_id,
                        actor,
                    )
                    .await
            }
            "unfreeze" => {
                service
                    .unfreeze_stage(order_id, &input.apparatus, &input.stage_node_id, actor)
                    .await
            }
            _ => return Err(bad_request("order_control_action_invalid")),
        }
        .map_err(production_map_error)?;
        return Ok(json_response(
            serde_json::json!({"ok":true,"scope":"stage","control":control}),
        ));
    }
    match input.action.trim() {
        "close_early" => {
            if input.comment.trim().is_empty() || input.comment.trim().chars().count() > 2000 {
                return Err(bad_request("early_close_comment_required_or_too_long"));
            }
            let control = state
                .production_maps
                .request_order_early_close(order_id, actor, &input.comment)
                .await
                .map_err(production_map_error)?;
            Ok(json_response(
                serde_json::json!({"ok": true, "control": control}),
            ))
        }
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
