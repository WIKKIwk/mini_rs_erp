use super::*;
use crate::core::production_map::pechat;
use crate::core::returned_paint::{
    ReturnedPaintItem, ReturnedPaintRequestCreate, returned_paint_astatka_total,
};

#[derive(serde::Deserialize)]
struct ApparatusQueueActionRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    material_barcode: String,
    #[serde(default)]
    material_barcodes: Vec<String>,
    #[serde(default)]
    qolip_code: String,
    #[serde(default)]
    produced_qty: Option<f64>,
    #[serde(default)]
    qty: Option<f64>,
    #[serde(default)]
    gross_qty: Option<f64>,
    #[serde(default)]
    return_ink_kg: Option<f64>,
    #[serde(default)]
    lamination_print_leftover_rolls: Option<f64>,
    #[serde(default)]
    lamination_film_leftover_rolls: Option<f64>,
    #[serde(default)]
    rezka_bosma_waste: Option<f64>,
    #[serde(default)]
    rezka_lamination_waste: Option<f64>,
    #[serde(default)]
    rezka_edge_waste: Option<f64>,
    #[serde(default)]
    total_waste: Option<f64>,
    #[serde(default)]
    finished_goods_kg: Option<f64>,
    #[serde(default)]
    finished_goods_meter: Option<f64>,
    #[serde(default)]
    uom: String,
    #[serde(default)]
    unit: String,
    #[serde(default)]
    progress_batch_id: String,
    #[serde(default)]
    progress_qr: String,
    #[serde(default)]
    qr_payload: String,
    #[serde(default)]
    driver_url: String,
    #[serde(default)]
    printer: String,
    #[serde(default)]
    print_mode: String,
    #[serde(default)]
    print_count: u32,
    #[serde(default)]
    completion_request_note: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    returned_paint_items: Vec<ReturnedPaintItem>,
    action: queue_state::ApparatusQueueAction,
}

pub async fn production_map_queue_action(
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
    let input: ApparatusQueueActionRequest = parse_json(&body)?;
    if input.apparatus.trim().is_empty() || input.order_id.trim().is_empty() {
        return Err(bad_request("apparatus and order_id are required"));
    }
    let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
    let material_barcodes = input.material_barcodes.clone();
    let material_barcode = if material_barcodes.is_empty() {
        input.material_barcode.clone()
    } else {
        material_barcodes.join(",")
    };
    let produced_qty = input.produced_qty.or(input.qty);
    let completion_request_note = if input.completion_request_note.trim().is_empty() {
        input.description.clone()
    } else {
        input.completion_request_note.clone()
    };
    if !matches!(input.action, queue_state::ApparatusQueueAction::Complete)
        && !input.returned_paint_items.is_empty()
    {
        return Err(bad_request("returned_paint_only_on_complete"));
    }
    let _queue_action_guard = state.production_maps.queue_action_guard().await;
    let returned_paint_report = if matches!(input.action, queue_state::ApparatusQueueAction::Complete)
        && !input.returned_paint_items.is_empty()
    {
        let map = state
            .production_maps
            .raw_map(&input.order_id)
            .await
            .map_err(production_map_error)?
            .ok_or_else(|| production_map_error(ProductionMapError::MapNotFound))?;
        let order_code = if map.code.trim().is_empty() {
            map.order_number.clone()
        } else {
            map.code.clone()
        };
        Some(
            state
                .returned_paint
                .prepare_request(
                    ReturnedPaintRequestCreate {
                        order_id: map.id,
                        order_code,
                        order_name: map.title,
                        apparatus: input.apparatus.clone(),
                        items: input.returned_paint_items.clone(),
                    },
                    &principal,
                    format!(
                        "returned_paint_complete:{}:{}",
                        input.order_id.trim(),
                        input.apparatus.trim()
                    ),
                )
                .map_err(|error| bad_request(error.to_string()))?,
        )
    } else {
        None
    };
    let return_ink_kg = match &returned_paint_report {
        Some(report) => Some(
            returned_paint_astatka_total(&report.items)
                .map_err(|error| bad_request(error.to_string()))?,
        ),
        None => input.return_ink_kg,
    };
    let progress = QueueProgressInput {
        produced_qty,
        uom: if input.uom.trim().is_empty() {
            input.unit.clone()
        } else {
            input.uom.clone()
        },
        progress_batch_id: input.progress_batch_id.clone(),
        qr_payload: if input.qr_payload.trim().is_empty() {
            input.progress_qr.clone()
        } else {
            input.qr_payload.clone()
        },
        return_ink_kg,
        lamination_print_leftover_rolls: input.lamination_print_leftover_rolls,
        lamination_film_leftover_rolls: input.lamination_film_leftover_rolls,
        rezka_bosma_waste: input.rezka_bosma_waste,
        rezka_lamination_waste: input.rezka_lamination_waste,
        rezka_edge_waste: input.rezka_edge_waste,
        total_waste: input.total_waste,
        finished_goods_kg: input.finished_goods_kg,
        finished_goods_meter: input.finished_goods_meter,
        description: completion_request_note.clone(),
    };
    let has_complete_bosma_metrics = return_ink_kg.is_some()
        && input.total_waste.is_some()
        && input.finished_goods_kg.is_some()
        && input.finished_goods_meter.is_some();
    let has_complete_laminatsiya_metrics = (input.lamination_print_leftover_rolls.is_some()
        || input.lamination_film_leftover_rolls.is_some())
        && input.total_waste.is_some()
        && input.finished_goods_kg.is_some()
        && input.finished_goods_meter.is_some();
    let has_rezka_progress_metrics = input.rezka_bosma_waste.is_some()
        && input.rezka_lamination_waste.is_some()
        && input.rezka_edge_waste.is_some();
    let zero_metric_codes = zero_completion_metric_codes(&input, return_ink_kg);
    if matches!(input.action, queue_state::ApparatusQueueAction::Complete)
        && !zero_metric_codes.is_empty()
        && completion_request_note.trim().is_empty()
    {
        return Err(bad_request("zero_metric_explanation_required"));
    }
    let missing_output_with_explanation = !has_complete_bosma_metrics
        && !has_complete_laminatsiya_metrics
        && !has_rezka_progress_metrics
        && input.gross_qty.is_none()
        && !completion_request_note.trim().is_empty();
    if matches!(input.action, queue_state::ApparatusQueueAction::Complete)
        && (!zero_metric_codes.is_empty() || missing_output_with_explanation)
    {
        let result = state
            .production_maps
            .request_completion_with_issue(
                &input.apparatus,
                &input.order_id,
                &assigned_apparatus,
                queue_action_actor(&principal),
                &completion_request_note,
                zero_metric_codes,
            )
            .await
            .map_err(production_map_error)?;
        return Ok(json_response(serde_json::json!({
            "ok": true,
            "states": result.states,
            "session": null,
            "progress_event": null,
            "progress_batch": null,
            "print": null,
            "completion_request": result.completion_request,
        })));
    }
    let mut prepared = state
        .production_maps
        .prepare_apparatus_queue_action_with_material_scan_and_progress(
            MaterialScanProgressAction {
                apparatus: &input.apparatus,
                order_id: &input.order_id,
                action: input.action,
                assigned_apparatus: &assigned_apparatus,
                actor: queue_action_actor(&principal),
                material_barcode: &material_barcode,
                progress,
            },
        )
        .await
        .map_err(production_map_error)?;
    let qolip_preparation = if matches!(input.action, queue_state::ApparatusQueueAction::Start) {
        prepare_qolip_for_bosma_start(&state, &principal, &input).await?
    } else {
        None
    };
    if let Some(preparation) = &qolip_preparation {
        prepared.attach_qolip_code(&preparation.spec.qolip_code);
    }
    let qolip_checkout = qolip_preparation.and_then(|preparation| preparation.checkout);
    let mut raw_material_stock_transitions = Vec::new();
    if matches!(input.action, queue_state::ApparatusQueueAction::Start) {
        let material_stock_barcodes = material_barcode
            .split(',')
            .map(|barcode| barcode.trim().to_string())
            .filter(|barcode| !barcode.is_empty())
            .collect::<Vec<_>>();
        if !material_stock_barcodes.is_empty() {
            raw_material_stock_transitions.push(RawMaterialStockTransition::new(
                RawMaterialStockTransitionKind::InUse,
                material_stock_barcodes,
                &input.order_id,
            ));
        }
    }
    let completed_material_barcodes =
        if matches!(input.action, queue_state::ApparatusQueueAction::Complete) {
            raw_material_barcodes_for_order_apparatus(&state, &input.order_id, &input.apparatus)
                .await?
        } else {
            Vec::new()
        };
    if !completed_material_barcodes.is_empty() {
        raw_material_stock_transitions.push(RawMaterialStockTransition::new(
            RawMaterialStockTransitionKind::Consumed,
            completed_material_barcodes,
            &input.order_id,
        ));
    }
    let print_request = prepared.progress_batch().and_then(|batch| {
        if matches!(
            input.action,
            queue_state::ApparatusQueueAction::Pause | queue_state::ApparatusQueueAction::Complete
        ) {
            Some(ProgressLabelPrintRequest {
                driver_url: input.driver_url.clone(),
                qr_payload: batch.qr_payload.clone(),
                item_code: batch.label_item_code.clone(),
                item_name: batch.label_item_name.clone(),
                executor_name: batch.executor_name.clone(),
                printer: input.printer.clone(),
                print_mode: input.print_mode.clone(),
                gross_qty: input
                    .gross_qty
                    .or(input.finished_goods_kg)
                    .unwrap_or(batch.produced_qty),
                progress_qty: batch.produced_qty,
                unit: "kg".to_string(),
                progress_unit: if batch.uom.trim().is_empty() {
                    "m".to_string()
                } else {
                    batch.uom.clone()
                },
                label_kind: String::new(),
                print_count: input.print_count,
            })
        } else {
            None
        }
    });
    let fallback_qolip_checkout = qolip_checkout.clone();
    let result = state
        .production_maps
        .commit_prepared_queue_action_with_raw_material_stock(
            prepared,
            raw_material_stock_transitions.clone(),
            qolip_checkout,
            returned_paint_report,
        )
        .await
        .map_err(production_map_error)?;
    if !result.qolip_checkout_committed
        && let Some(checkout) = fallback_qolip_checkout
    {
        state
            .qolip
            .issue_prepared_checkout(checkout)
            .await
            .map_err(qolip_queue_error)?;
    }
    let mut warehouse_stock_update_warehouses = result.raw_material_stock_warehouses.clone();
    if !raw_material_stock_transitions.is_empty() && warehouse_stock_update_warehouses.is_empty() {
        for transition in &raw_material_stock_transitions {
            let updates = match transition.kind {
                RawMaterialStockTransitionKind::InUse => {
                    state
                        .gscale
                        .mark_raw_material_stock_in_use(&transition.barcodes, &transition.order_id)
                        .await
                }
                RawMaterialStockTransitionKind::Consumed => {
                    state
                        .gscale
                        .mark_raw_material_stock_consumed(
                            &transition.barcodes,
                            &transition.order_id,
                        )
                        .await
                }
            }
            .map_err(raw_material_stock_status_error)?;
            warehouse_stock_update_warehouses.extend(
                updates
                    .into_iter()
                    .map(|stock| stock.warehouse)
                    .filter(|warehouse| !warehouse.trim().is_empty()),
            );
        }
    }
    for warehouse in warehouse_stock_update_warehouses {
        state
            .warehouse_events
            .notify_updated(&warehouse, "raw_material_stock");
    }
    let mut print = serde_json::Value::Null;
    if let Some(request) = print_request {
        match state.gscale.print_progress_label(request).await {
            Ok(response) => {
                print = serde_json::to_value(response).unwrap_or(serde_json::Value::Null);
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    apparatus = %input.apparatus,
                    order_id = %input.order_id,
                    action = ?input.action,
                    "progress label print failed after queue action commit"
                );
                print = progress_print_failure_json(error);
            }
        }
    }
    Ok(json_response(serde_json::json!({
        "ok": true,
        "states": result.states,
        "order_status": result.order_status,
        "session": result.session,
        "progress_event": result.progress_event,
        "progress_batch": result.progress_batch,
        "print": print,
    })))
}

fn zero_completion_metric_codes(
    input: &ApparatusQueueActionRequest,
    return_ink_kg: Option<f64>,
) -> Vec<String> {
    if !matches!(input.action, queue_state::ApparatusQueueAction::Complete) {
        return Vec::new();
    }
    [
        ("produced_qty", input.produced_qty.or(input.qty)),
        ("gross_qty", input.gross_qty),
        ("return_ink_kg", return_ink_kg),
        (
            "lamination_print_leftover_rolls",
            input.lamination_print_leftover_rolls,
        ),
        (
            "lamination_film_leftover_rolls",
            input.lamination_film_leftover_rolls,
        ),
        ("rezka_bosma_waste", input.rezka_bosma_waste),
        ("rezka_lamination_waste", input.rezka_lamination_waste),
        ("rezka_edge_waste", input.rezka_edge_waste),
        ("total_waste", input.total_waste),
        ("finished_goods_kg", input.finished_goods_kg),
        ("finished_goods_meter", input.finished_goods_meter),
    ]
    .into_iter()
    .filter_map(|(code, value)| {
        value
            .is_some_and(|value| value == 0.0)
            .then_some(code.to_string())
    })
    .collect()
}

async fn prepare_qolip_for_bosma_start(
    state: &AppState,
    principal: &Principal,
    input: &ApparatusQueueActionRequest,
) -> Result<Option<crate::core::qolip::QolipOrderStartPreparation>, AdminError> {
    if !apparatus_requires_qolip_scan(&input.apparatus) {
        return Ok(None);
    }
    let Some(map) = state
        .production_maps
        .raw_map(&input.order_id)
        .await
        .map_err(production_map_error)?
    else {
        return Err(production_map_error(ProductionMapError::MapNotFound));
    };
    let qolip_code = input.qolip_code.trim();
    if qolip_code.is_empty() {
        return Err(bad_request("qolip_scan_required"));
    }
    let preparation = state
        .qolip
        .prepare_qolip_code_for_order_start(
            qolip_code,
            &map.product_code,
            &map.title,
            &principal.ref_,
            &principal.display_name,
            principal,
        )
        .await
        .map_err(qolip_queue_error)?;
    reject_qolip_in_use(
        state,
        &input.apparatus,
        &input.order_id,
        &preparation.spec.qolip_code,
    )
    .await?;
    Ok(Some(preparation))
}

pub(super) async fn reject_qolip_in_use(
    state: &AppState,
    apparatus: &str,
    order_id: &str,
    qolip_code: &str,
) -> Result<(), AdminError> {
    let active = state
        .production_maps
        .active_order_run_session_for_qolip(qolip_code)
        .await
        .map_err(production_map_error)?;
    if active.is_some_and(|session| {
        session.order_id.trim() != order_id.trim()
            || !queue_state::apparatus_titles_match(&session.apparatus, apparatus)
    }) {
        return Err(production_map_error(
            ProductionMapError::QolipAlreadyInUse,
        ));
    }
    Ok(())
}

pub(super) fn apparatus_requires_qolip_scan(apparatus: &str) -> bool {
    pechat::pechat_color_count(apparatus).is_some()
}

pub(super) fn qolip_queue_error(error: crate::core::qolip::QolipError) -> AdminError {
    match error {
        crate::core::qolip::QolipError::MissingQolipCode => bad_request("qolip_scan_required"),
        crate::core::qolip::QolipError::QolipCodeNotFound => bad_request("qolip_code_not_found"),
        crate::core::qolip::QolipError::QolipCodeMismatch => bad_request("qolip_code_mismatch"),
        crate::core::qolip::QolipError::LocationNotFound => bad_request("qolip_location_not_found"),
        crate::core::qolip::QolipError::InsufficientStock => bad_request("insufficient_stock"),
        crate::core::qolip::QolipError::LocationIdentityMismatch => {
            bad_request("location_identity_mismatch")
        }
        crate::core::qolip::QolipError::StoreFailed => server_error("qolip store failed"),
        other => bad_request(other.to_string()),
    }
}

pub(super) fn progress_print_failure_json(
    error: crate::core::gscale::GscaleServiceError,
) -> serde_json::Value {
    let (code, detail) = match error {
        crate::core::gscale::GscaleServiceError::PrintFailed { detail, .. } => {
            ("print_failed", clean_progress_print_error(&detail))
        }
        crate::core::gscale::GscaleServiceError::NotConfigured(_) => (
            "scale_driver_not_configured",
            "scale_driver_not_configured".to_string(),
        ),
        other => (other.code(), other.to_string()),
    };
    serde_json::json!({
        "ok": false,
        "status": "failed",
        "code": code,
        "error": detail,
    })
}

pub(super) fn clean_progress_print_error(detail: &str) -> String {
    detail
        .trim()
        .strip_prefix("driver request failed: ")
        .unwrap_or_else(|| detail.trim())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::apparatus_requires_qolip_scan;

    #[test]
    fn qolip_scan_is_required_only_for_seven_eight_and_nine_color_bosma_family() {
        assert!(apparatus_requires_qolip_scan("7 ta rangli pechat - A"));
        assert!(apparatus_requires_qolip_scan("8 ta rangli bosma aparat"));
        assert!(apparatus_requires_qolip_scan("9 rangli val"));
        assert!(!apparatus_requires_qolip_scan("Laminatsiya"));
        assert!(!apparatus_requires_qolip_scan("Rezka aparat"));
        assert!(!apparatus_requires_qolip_scan("Pechat"));
    }
}
