use super::raw_materials::{
    material_warehouse_scope, raw_material_stock_locked_error, warehouse_in_scope,
};
use super::*;

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMaterialStockReprintRequest {
    #[serde(default)]
    barcode: String,
}

pub async fn raw_material_stock_reprint_prepare(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::RawMaterialAssign, Capability::GscalePrint],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    require_material_reprint_capabilities(&state, &principal).await?;
    let request: RawMaterialStockReprintRequest = parse_json(&body)?;
    let stock = reprintable_material_stock(&state, &principal, &request.barcode).await?;
    let reprint_id = new_raw_material_reprint_id();
    Ok(json_response(serde_json::json!({
        "ok": true,
        "reprint_id": reprint_id,
        "stock": stock,
        "print": {
            "ok": true,
            "status": "prepared",
            "epc": stock.barcode,
            "item_code": stock.item_code,
            "item_name": stock.item_name,
            "warehouse": stock.warehouse,
            "qty": stock.qty,
            "net_qty": stock.qty,
            "gross_qty": stock.qty,
            "unit": stock.uom,
            "printer": "godex",
            "print_mode": "label",
            "label_kind": "material_product",
            "printer_status": "client_usb_pending",
            "print_count": 1,
        }
    })))
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMaterialStockReprintConfirmRequest {
    #[serde(default)]
    barcode: String,
    #[serde(default)]
    reprint_id: String,
}

pub async fn raw_material_stock_reprint_confirm(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::RawMaterialAssign, Capability::GscalePrint],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    require_material_reprint_capabilities(&state, &principal).await?;
    let request: RawMaterialStockReprintConfirmRequest = parse_json(&body)?;
    if !valid_raw_material_reprint_id(&request.reprint_id) {
        return Err(bad_request("raw_material_reprint_id_required"));
    }
    let stock = material_stock_in_scope(&state, &principal, &request.barcode).await?;
    record_raw_material_label_reprinted(&state, &principal, &stock, request.reprint_id.trim())
        .await;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "reprint_id": request.reprint_id.trim(),
        "barcode": stock.barcode,
    })))
}

async fn require_material_reprint_capabilities(
    state: &AppState,
    principal: &Principal,
) -> Result<(), AdminError> {
    if principal.role != PrincipalRole::MaterialTaminotchi {
        return Err(forbidden());
    }
    require_capability(state, principal, Capability::RawMaterialAssign).await?;
    require_capability(state, principal, Capability::GscalePrint).await
}

async fn reprintable_material_stock(
    state: &AppState,
    principal: &Principal,
    barcode: &str,
) -> Result<RawMaterialStockEntry, AdminError> {
    let stock = material_stock_in_scope(state, principal, barcode).await?;
    let has_assignment = state
        .production_maps
        .raw_material_assignments()
        .await
        .map_err(production_map_error)?
        .iter()
        .any(|assignment| {
            assignment
                .barcode
                .trim()
                .eq_ignore_ascii_case(stock.barcode.trim())
        });
    if has_assignment
        || !stock.status.trim().eq_ignore_ascii_case("available")
        || !stock.reserved_order_id.trim().is_empty()
    {
        return Err(raw_material_stock_locked_error());
    }
    Ok(stock)
}

async fn material_stock_in_scope(
    state: &AppState,
    principal: &Principal,
    barcode: &str,
) -> Result<RawMaterialStockEntry, AdminError> {
    let barcode = barcode.trim();
    if barcode.is_empty() {
        return Err(bad_request("raw_material_stock_reprint_invalid"));
    }
    let stock = state
        .gscale
        .raw_material_stock_by_barcode(barcode)
        .await
        .map_err(|_| server_error("raw material stock fetch failed"))?
        .ok_or_else(|| not_found("raw_material_stock_not_found"))?;
    let warehouses = material_warehouse_scope(state, principal).await?;
    if !warehouse_in_scope(&warehouses, &stock.warehouse) {
        return Err(forbidden());
    }
    Ok(stock)
}

async fn record_raw_material_label_reprinted(
    state: &AppState,
    principal: &Principal,
    stock: &RawMaterialStockEntry,
    reprint_id: &str,
) {
    let Some(engine) = state.mini_engine.as_ref() else {
        return;
    };
    let actor = queue_action_actor(principal);
    let event = crate::engine::EngineEventDraft {
        domain: "raw_material_stock".to_string(),
        action: "label_reprinted".to_string(),
        entity_id: stock.barcode.trim().to_string(),
        actor_key: format!("{}:{}", actor.role.trim(), actor.ref_.trim()),
        idempotency_key: reprint_id.trim().to_string(),
        payload_json: serde_json::json!({
            "barcode": stock.barcode.trim(),
            "source_receipt_id": stock.source_receipt_id.trim(),
            "warehouse": stock.warehouse.trim(),
            "item_code": stock.item_code.trim(),
            "item_name": stock.item_name.trim(),
            "qty": stock.qty,
            "uom": stock.uom.trim(),
            "reprint_id": reprint_id.trim(),
        }),
    };
    if let Err(error) = engine.record_event(&event).await {
        tracing::warn!(%error, "raw material label reprint audit failed");
    }
}

fn new_raw_material_reprint_id() -> String {
    let bytes: [u8; 16] = rand::random();
    format!("raw_label_{}", data_encoding::HEXLOWER.encode(&bytes))
}

fn valid_raw_material_reprint_id(value: &str) -> bool {
    let Some(suffix) = value.trim().strip_prefix("raw_label_") else {
        return false;
    };
    suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
}
