use super::queue_actions::{apparatus_requires_qolip_scan, qolip_queue_error, reject_qolip_in_use};
use super::*;

#[derive(serde::Deserialize)]
struct QolipStartValidationRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    qolip_code: String,
}

pub async fn production_map_qolip_validate(
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
    let input: QolipStartValidationRequest = parse_json(&body)?;
    let apparatus = input.apparatus.trim();
    let order_id = input.order_id.trim();
    if apparatus.is_empty() || order_id.is_empty() {
        return Err(bad_request("apparatus and order_id are required"));
    }
    if !apparatus_requires_qolip_scan(apparatus) {
        return Err(bad_request("qolip_scan_not_required"));
    }
    let is_admin = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await;
    let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
    if !is_admin && !queue_state::apparatus_matches_assigned(apparatus, &assigned_apparatus) {
        return Err(bad_request("apparatus_not_assigned"));
    }
    let Some(map) = state
        .production_maps
        .raw_map(order_id)
        .await
        .map_err(production_map_error)?
    else {
        return Err(production_map_error(ProductionMapError::MapNotFound));
    };
    let preparation = state
        .qolip
        .prepare_qolip_code_for_order_start(
            &input.qolip_code,
            &map.product_code,
            &map.title,
            &principal.ref_,
            &principal.display_name,
            &principal,
        )
        .await
        .map_err(qolip_queue_error)?;
    reject_qolip_in_use(&state, apparatus, order_id, &preparation.spec.qolip_code).await?;
    let block = preparation
        .checkout
        .as_ref()
        .map(|checkout| checkout.block.as_str())
        .unwrap_or_default();
    Ok(json_response(serde_json::json!({
        "ok": true,
        "qolip": {
            "qolip_code": preparation.spec.qolip_code,
            "item_code": preparation.spec.item_code,
            "item_name": preparation.spec.item_name,
            "item_group": preparation.spec.item_group,
            "size": preparation.spec.size,
            "block": block,
        }
    })))
}
