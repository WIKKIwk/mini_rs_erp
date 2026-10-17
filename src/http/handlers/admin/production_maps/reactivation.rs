use super::queue_actions::qolip_queue_error;
use super::*;
use crate::core::production_map::{ReactivationSelector, ReactivationTarget};

pub async fn production_map_reactivation_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal =
        authorize_capability(&state, &headers, Capability::ApparatusQueueManage).await?;
    if principal.role != PrincipalRole::Aparatchi {
        return Err(forbidden());
    }
    let selector: ReactivationSelector = parse_json(&body)?;
    let assigned = state.admin.principal_assigned_apparatus(&principal).await;
    let result = state
        .production_maps
        .preview_reactivation(&selector, &assigned, &queue_action_actor(&principal))
        .await
        .map_err(production_map_error)?;
    Ok(json_response(
        serde_json::to_value(result).map_err(|_| bad_request("invalid_reactivation"))?,
    ))
}

pub async fn production_map_reactivation(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal =
        authorize_capability(&state, &headers, Capability::ApparatusQueueManage).await?;
    if principal.role != PrincipalRole::Aparatchi {
        return Err(forbidden());
    }
    let target: ReactivationTarget = parse_json(&body)?;
    let assigned = state.admin.principal_assigned_apparatus(&principal).await;
    let actor = queue_action_actor(&principal);
    if state.production_maps.reactivation_was_applied(&target, &assigned, &actor)
        .await.map_err(production_map_error)?
    {
        return Ok(reactivation_ack(&state));
    }
    // Validate scope before reading resources or preparing any checkout. A replay
    // can acknowledge its existing receipt without attempting tooling again.
    let selector = ReactivationSelector {
        apparatus: target.apparatus.clone(),
        order_id: target.order_id.clone(),
        batch_id: target.batch_id.clone(),
    };
    let preview = state
        .production_maps
        .preview_reactivation(&selector, &assigned, &actor)
        .await
        .map_err(production_map_error)?;
    let mut checkouts = Vec::new();
    if preview.available {
        let current = preview
            .target
            .ok_or_else(|| bad_request("invalid_reactivation"))?;
        if current.token != target.token
            || current.session_id != target.session_id
            || current.stage_node_id != target.stage_node_id
        {
            // Another request may have committed while preview was loading.
            if state.production_maps.reactivation_was_applied(&target, &assigned, &actor)
                .await.map_err(production_map_error)?
            {
                return Ok(reactivation_ack(&state));
            }
            return Err(production_map_error(
                ProductionMapError::ReactivationDenied("stale_target".into()),
            ));
        }
        let session = state
            .production_maps
            .order_run_sessions_for_order(&target.order_id)
            .await
            .map_err(production_map_error)?
            .into_iter()
            .find(|s| s.session_id == target.session_id)
            .ok_or_else(|| production_map_error(ProductionMapError::QueueActionNotAllowed))?;
        if target.batch_id.is_empty() {
            let map = state
                .production_maps
                .raw_map(&target.order_id)
                .await
                .map_err(production_map_error)?
                .ok_or_else(|| production_map_error(ProductionMapError::MapNotFound))?;
            let mut codes = session
                .payload_json
                .get("qolip_codes")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if codes.is_empty() {
                if let Some(code) = session
                    .payload_json
                    .get("qolip_code")
                    .and_then(serde_json::Value::as_str)
                    .filter(|code| !code.is_empty())
                {
                    codes.push(code.into());
                }
            }
            for code in codes {
                let preparation = state
                    .qolip
                    .prepare_qolip_code_for_order_start(
                        &code,
                        &map.product_code,
                        &map.title,
                        &principal.ref_,
                        &principal.display_name,
                        &principal,
                    )
                    .await
                    .map_err(qolip_queue_error)?;
                if let Some(checkout) = preparation.checkout {
                    checkouts.push(checkout);
                }
            }
        }
    }
    state
        .production_maps
        .reactivate_completed_work(&target, &assigned, &actor, &checkouts)
        .await
        .map_err(production_map_error)?;
    if let Ok(barcodes) =
        raw_material_barcodes_for_order_apparatus(&state, &target.order_id, &target.apparatus).await
    {
        let mut warehouses = std::collections::BTreeSet::new();
        for barcode in barcodes {
            if let Ok(Some(stock)) = state.gscale.raw_material_stock_by_barcode(&barcode).await {
                warehouses.insert(stock.warehouse);
            }
        }
        for warehouse in warehouses {
            state
                .warehouse_events
                .notify_updated(&warehouse, "raw_material_stock");
        }
    }
    Ok(reactivation_ack(&state))
}

fn reactivation_ack(state: &AppState) -> Response {
    json_response(serde_json::json!({"ok":true,"rev":state.production_maps.snapshot_revision(),"epoch":state.production_maps.snapshot_epoch()}))
}
