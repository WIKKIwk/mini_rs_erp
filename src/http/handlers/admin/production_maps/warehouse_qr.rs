use std::collections::BTreeMap;

use super::*;
use crate::core::production_map::{OrderProgressBatch, WarehouseWipSnapshot};

#[derive(serde::Deserialize)]
pub struct WarehouseQrQuery {
    qr_payload: String,
}

async fn authorize_warehouse_scan(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(Principal, Vec<String>), AdminError> {
    let principal = authorize_any_capability(state, headers, &[Capability::WerkaAccess]).await?;
    if principal.role != PrincipalRole::Werka {
        return Err(forbidden());
    }
    let warehouses = state
        .warehouses
        .assigned_warehouse_names(&principal)
        .await
        .map_err(warehouse_error)?;
    if warehouses.is_empty() {
        return Err(forbidden());
    }
    Ok((principal, warehouses))
}

fn check_receipt_scope(
    snapshot: &WarehouseWipSnapshot,
    warehouses: &[String],
) -> Result<(), AdminError> {
    if let Some(receipt) = snapshot.receipt() {
        if !warehouses.contains(&receipt.warehouse) {
            return Err(forbidden());
        }
    }
    Ok(())
}

async fn apparatus_names(
    state: &AppState,
    batches: &[OrderProgressBatch],
) -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    for batch in batches {
        for id in [
            &batch.apparatus,
            &batch.current_apparatus,
            &batch.next_apparatus,
        ] {
            if id.trim().is_empty() || names.contains_key(id) {
                continue;
            }
            // Display enrichment is optional. Never grant admin catalog access or
            // let an old/retired display name turn into receive authorization.
            if let Ok(canonical) = state
                .production_maps
                .resolve_canonical_apparatus_text(id)
                .await
            {
                names.insert(id.clone(), canonical.runtime.display.display_name.clone());
            }
        }
    }
    names
}

pub async fn werka_qr_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WarehouseQrQuery>,
) -> Result<Response, AdminError> {
    let (_, warehouses) = authorize_warehouse_scan(&state, &headers).await?;
    let qr = query.qr_payload.trim();
    if qr.is_empty() || qr.len() > 2048 {
        return Err(bad_request("progress_input_invalid"));
    }
    let wip = state
        .production_maps
        .warehouse_wip_snapshot(qr)
        .await
        .map_err(production_map_error)?;
    let paddon = match state.production_maps.paddon_scan_snapshot(qr).await {
        Ok(value) => Some(value),
        Err(ProductionMapError::PaddonNotFound) => None,
        Err(error) => return Err(production_map_error(error)),
    };
    if wip.as_ref().is_some_and(|s| s.qr_is_paddon) || (wip.is_some() && paddon.is_some()) {
        return Err(conflict("qr_ambiguous"));
    }
    if let Some(snapshot) = paddon {
        let receipt = state
            .production_maps
            .paddon_receipt(qr)
            .await
            .map_err(production_map_error)?;
        if receipt
            .as_ref()
            .is_some_and(|r| !warehouses.contains(&r.warehouse))
        {
            return Err(forbidden());
        }
        let can_receive = if receipt.is_some() {
            false
        } else {
            match state
                .production_maps
                .validate_paddon_receiving_items(&snapshot.items)
                .await
            {
                Ok(()) => true,
                Err(
                    ProductionMapError::PaddonInvalidInput
                    | ProductionMapError::ProgressBatchNotAccepted
                    | ProductionMapError::ProgressInputInvalid
                    | ProductionMapError::MapNotFound,
                ) => false,
                Err(error) => return Err(production_map_error(error)),
            }
        };
        let names = apparatus_names(&state, &snapshot.items).await;
        return Ok(json_response(serde_json::json!({
            "kind": "paddon", "paddon": snapshot.paddon, "items": snapshot.items,
            "snapshot_token": crate::core::production_map::paddon_snapshot_token(&snapshot),
            "warehouses": warehouses, "can_receive": can_receive, "receipt": receipt,
            "apparatus_names": names,
        })));
    }
    if let Some(snapshot) = wip {
        check_receipt_scope(&snapshot, &warehouses)?;
        let names = apparatus_names(&state, std::slice::from_ref(&snapshot.batch)).await;
        return Ok(json_response(serde_json::json!({
            "kind": "wip", "batch": snapshot.batch, "warehouses": warehouses,
            "can_receive": snapshot.receive_blocked_reason().is_none(),
            "receive_blocked_reason": snapshot.receive_blocked_reason(),
            "snapshot_token": snapshot.snapshot_token(), "receipt": snapshot.receipt(),
            "paddon_code": snapshot.paddon_code, "apparatus_names": names,
        })));
    }
    Err(not_found("qr_not_found"))
}

#[derive(serde::Deserialize)]
pub struct WarehouseWipReceiveRequest {
    progress_batch_id: String,
    qr_payload: String,
    warehouse: String,
    snapshot_token: String,
}

pub async fn werka_wip_receive(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<WarehouseWipReceiveRequest>,
) -> Result<Response, AdminError> {
    let (principal, warehouses) = authorize_warehouse_scan(&state, &headers).await?;
    let warehouse =
        super::wip::assigned_finished_goods_warehouse(&state, &principal, &input.warehouse).await?;
    // Do not expose another warehouse's receipt even on a rejected replay.
    let snapshot = state
        .production_maps
        .warehouse_wip_snapshot(&input.qr_payload)
        .await
        .map_err(production_map_error)?
        .ok_or_else(|| not_found("qr_not_found"))?;
    check_receipt_scope(&snapshot, &warehouses)?;
    let (batch, receipt) = state
        .production_maps
        .receive_warehouse_wip(
            &input.progress_batch_id,
            &input.qr_payload,
            &warehouse,
            &input.snapshot_token,
            queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    state
        .warehouse_events
        .notify_updated(&warehouse, "finished_goods_stock");
    Ok(json_response(
        serde_json::json!({"ok": true, "batch": batch, "receipt": receipt}),
    ))
}
