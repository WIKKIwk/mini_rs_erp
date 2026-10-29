use super::*;
use crate::core::production_map::{LayerRemainderInput, LayerRemainderRecord};

#[derive(serde::Deserialize)]
pub struct LayerRemainderQuery {
    apparatus: String,
    order_id: String,
}

#[derive(Default, serde::Deserialize)]
pub(super) struct LayerRemainderPrintOptions {
    #[serde(default)]
    pub(super) driver_url: String,
    #[serde(default)]
    pub(super) printer: String,
    #[serde(default)]
    pub(super) print_mode: String,
    #[serde(default)]
    pub(super) print_transport: String,
    #[serde(default)]
    pub(super) print_count: u32,
}

#[derive(serde::Deserialize)]
struct LayerRemainderRequest {
    #[serde(flatten)]
    remainder: LayerRemainderInput,
    #[serde(flatten)]
    print: LayerRemainderPrintOptions,
}

pub async fn production_map_layer_remainders(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    uri: axum::http::Uri,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = if method == Method::GET {
        authorize_any_capability(
            &state,
            &headers,
            &[
                Capability::AdminAccess,
                Capability::ProductionMapManage,
                Capability::ProductionMapRead,
                Capability::ApparatusQueueManage,
            ],
        )
        .await?
    } else {
        layer_remainder_principal(&state, &headers).await?
    };
    if method == Method::GET {
        let Query(query) = Query::<LayerRemainderQuery>::try_from_uri(&uri)
            .map_err(|_| bad_request("apparatus and order_id are required"))?;
        layer_remainder_scope(&state, &principal, &query.apparatus).await?;
        let sources = state
            .production_maps
            .layer_remainder_sources(&query.apparatus, &query.order_id)
            .await
            .map_err(production_map_error)?;
        let remainders = state
            .production_maps
            .layer_remainder_history(&query.apparatus, &query.order_id)
            .await
            .map_err(production_map_error)?;
        return Ok(json_response(serde_json::json!({
            "ok": true, "sources": sources, "remainders": remainders,
        })));
    }
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: LayerRemainderRequest = parse_json(&body)?;
    layer_remainder_scope(&state, &principal, &input.remainder.apparatus).await?;
    validate_layer_print(&input.print)?;
    // Commit the stock operation first. A failed printer can only retry this
    // saved label; it must never create another remainder or supplier receipt.
    let remainder = state
        .production_maps
        .record_layer_remainder(input.remainder, queue_action_actor(&principal))
        .await
        .map_err(production_map_error)?;
    if let Some(stock) = &remainder.raw_material_stock {
        state
            .warehouse_events
            .notify_updated(&stock.warehouse, "layer_remainder_recorded");
    }
    layer_remainder_response(&state, remainder, &input.print).await
}

#[derive(serde::Deserialize)]
struct LayerRemainderReprintRequest {
    apparatus: String,
    order_id: String,
    operation_id: String,
    #[serde(flatten)]
    print: LayerRemainderPrintOptions,
}

pub async fn production_map_layer_remainder_reprint(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = layer_remainder_principal(&state, &headers).await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: LayerRemainderReprintRequest = parse_json(&body)?;
    layer_remainder_scope(&state, &principal, &input.apparatus).await?;
    validate_layer_print(&input.print)?;
    let remainder = state
        .production_maps
        .layer_remainder_by_id(&input.operation_id)
        .await
        .map_err(production_map_error)?
        .ok_or_else(|| bad_request("layer_remainder_not_found"))?;
    if remainder.apparatus != input.apparatus.trim() || remainder.order_id != input.order_id.trim()
    {
        return Err(forbidden());
    }
    if remainder.remaining_net_kg <= 0.0 || remainder.qr_payload.trim().is_empty() {
        return Err(bad_request("layer_remainder_label_unavailable"));
    }
    layer_remainder_response(&state, remainder, &input.print).await
}

async fn layer_remainder_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, AdminError> {
    authorize_any_capability(
        state,
        headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueManage,
        ],
    )
    .await
}

async fn layer_remainder_scope(
    state: &AppState,
    principal: &Principal,
    apparatus: &str,
) -> Result<(), AdminError> {
    if apparatus.trim().is_empty() {
        return Err(bad_request("apparatus is required"));
    }
    if !state
        .admin
        .principal_has_admin_workspace_access(principal)
        .await
        && !state
            .admin
            .principal_has_capability(principal, Capability::ProductionMapRead)
            .await
    {
        let assigned = state.admin.principal_assigned_apparatus(principal).await;
        if !queue_state::apparatus_matches_assigned(apparatus, &assigned) {
            return Err(production_map_error(
                ProductionMapError::ApparatusNotAssigned,
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_layer_print(input: &LayerRemainderPrintOptions) -> Result<(), AdminError> {
    if input.print_count > 1 {
        return Err(bad_request("layer_remainder_single_label_required"));
    }
    Ok(())
}

async fn layer_remainder_response(
    state: &AppState,
    remainder: LayerRemainderRecord,
    options: &LayerRemainderPrintOptions,
) -> Result<Response, AdminError> {
    let print = layer_remainder_print(state, &remainder, options).await?;
    Ok(json_response(
        serde_json::json!({"ok": true, "remainder": remainder, "print": print}),
    ))
}

pub(super) async fn layer_remainder_print(
    state: &AppState,
    remainder: &LayerRemainderRecord,
    options: &LayerRemainderPrintOptions,
) -> Result<serde_json::Value, AdminError> {
    let mut request = ProgressLabelPrintRequest {
        driver_url: options.driver_url.clone(),
        printer: options.printer.clone(),
        print_mode: options.print_mode.clone(),
        print_count: 1,
        qr_payload: remainder.qr_payload.clone(),
        gross_qty: remainder.remaining_net_kg,
        progress_qty: remainder.remaining_meter,
        unit: "kg".into(),
        progress_unit: "m".into(),
        ..Default::default()
    };
    if let Some(batch) = &remainder.progress_batch {
        request.item_code = batch.label_item_code.clone();
        request.item_name = batch.label_item_name.clone();
        request.apparatus = batch.apparatus.clone();
        request.apparatus_display_name = state
            .production_maps
            .resolve_canonical_apparatus_text(&batch.apparatus)
            .await
            .map_err(production_map_error)?
            .runtime
            .display
            .display_name
            .clone();
        request.customer_name = batch.payload_json["customer_name"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        request.executor_name = batch.executor_name.clone();
        request.tare_kg = batch.bobina_kg.unwrap_or(0.0);
        request.tare_enabled = request.tare_kg > 0.0;
        request.gross_qty = remainder.remaining_net_kg + request.tare_kg;
        request.label_kind = "progress".into();
    } else if let Some(stock) = &remainder.raw_material_stock {
        request.item_code = stock.item_code.clone();
        request.item_name = stock.item_name.clone();
        if let (Some(width), Some(micron)) = (stock.width_mm, stock.micron) {
            let compact = |value: f64| {
                format!("{value:.6}")
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            };
            let dimensions = format!("{}/{}", compact(width), compact(micron));
            if !request.item_name.trim().ends_with(&dimensions) {
                request.item_name = format!("{} {}", request.item_name.trim(), dimensions);
            }
        }
        request.apparatus.clear();
        request.apparatus_display_name = stock.warehouse.clone();
        request.executor_name = remainder.producer_name.clone();
        request.tare_kg = remainder.tare_kg;
        request.tare_enabled = request.tare_kg > 0.0;
        request.gross_qty = remainder.remaining_net_kg + request.tare_kg;
        request.label_kind = "material_product".into();
    } else {
        return Err(production_map_error(ProductionMapError::StoreFailed));
    }
    let result = if options
        .print_transport
        .trim()
        .eq_ignore_ascii_case("offline")
    {
        state.gscale.prepare_progress_label(request)
    } else if let Some(stock) = &remainder.raw_material_stock {
        state
            .gscale
            .print_existing_material_label(request, &stock.warehouse)
            .await
    } else {
        state.gscale.print_progress_label(request).await
    };
    let mut print = match result {
        Ok(response) => serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
        Err(error) => super::queue_actions::progress_print_failure_json(error),
    };
    if print.is_object() {
        print["qr_payload"] = serde_json::json!(remainder.qr_payload);
        print["net_qty"] = serde_json::json!(remainder.remaining_net_kg);
        print["progress_qty"] = serde_json::json!(remainder.remaining_meter);
        if let Some(stock) = &remainder.raw_material_stock {
            print["qty"] = serde_json::json!(remainder.remaining_net_kg);
            print["epc"] = serde_json::json!(stock.barcode);
            print["warehouse"] = serde_json::json!(stock.warehouse);
            print["width_mm"] = serde_json::json!(stock.width_mm);
            print["micron"] = serde_json::json!(stock.micron);
            print["length_m"] = serde_json::json!(stock.length_m);
        }
    }
    Ok(print)
}
