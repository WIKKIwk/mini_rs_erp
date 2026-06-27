use super::*;

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
        return_ink_kg: input.return_ink_kg,
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
    let _queue_action_guard = state.production_maps.queue_action_guard().await;
    let has_complete_bosma_metrics = input.return_ink_kg.is_some()
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
    if matches!(input.action, queue_state::ApparatusQueueAction::Complete)
        && !has_complete_bosma_metrics
        && !has_complete_laminatsiya_metrics
        && !has_rezka_progress_metrics
        && input.gross_qty.is_none()
        && !completion_request_note.trim().is_empty()
    {
        let result = state
            .production_maps
            .request_completion_without_output(
                &input.apparatus,
                &input.order_id,
                &assigned_apparatus,
                queue_action_actor(&principal),
                &completion_request_note,
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
    let prepared = state
        .production_maps
        .prepare_apparatus_queue_action_with_material_scan_and_progress(
            &input.apparatus,
            &input.order_id,
            input.action,
            &assigned_apparatus,
            queue_action_actor(&principal),
            &material_barcode,
            progress,
        )
        .await
        .map_err(production_map_error)?;
    let mut warehouse_stock_updates = Vec::new();
    if matches!(input.action, queue_state::ApparatusQueueAction::Start) {
        let material_stock_barcodes = material_barcode
            .split(',')
            .map(|barcode| barcode.trim().to_string())
            .filter(|barcode| !barcode.is_empty())
            .collect::<Vec<_>>();
        if !material_stock_barcodes.is_empty() {
            warehouse_stock_updates.extend(
                state
                    .gscale
                    .mark_raw_material_stock_in_use(&material_stock_barcodes, &input.order_id)
                    .await
                    .map_err(raw_material_stock_status_error)?,
            );
        }
    }
    let completed_material_barcodes =
        if matches!(input.action, queue_state::ApparatusQueueAction::Complete) {
            raw_material_barcodes_for_order_apparatus(&state, &input.order_id, &input.apparatus)
                .await?
        } else {
            Vec::new()
        };
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
    let result = state
        .production_maps
        .commit_prepared_queue_action(prepared)
        .await
        .map_err(production_map_error)?;
    if !completed_material_barcodes.is_empty() {
        warehouse_stock_updates.extend(
            state
                .gscale
                .mark_raw_material_stock_consumed(&completed_material_barcodes, &input.order_id)
                .await
                .map_err(raw_material_stock_status_error)?,
        );
    }
    for stock in warehouse_stock_updates {
        state
            .warehouse_events
            .notify_updated(&stock.warehouse, "raw_material_stock");
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

fn progress_print_failure_json(
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

fn clean_progress_print_error(detail: &str) -> String {
    detail
        .trim()
        .strip_prefix("driver request failed: ")
        .unwrap_or_else(|| detail.trim())
        .to_string()
}

#[derive(serde::Deserialize)]
struct ProgressQrLookupRequest {
    #[serde(default)]
    progress_batch_id: String,
    #[serde(default)]
    progress_qr: String,
    #[serde(default)]
    qr_payload: String,
}

pub async fn production_map_progress_qr_lookup(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
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
    let input: ProgressQrLookupRequest = parse_json(&body)?;
    let qr_payload = if input.qr_payload.trim().is_empty() {
        input.progress_qr
    } else {
        input.qr_payload
    };
    let batch = state
        .production_maps
        .progress_batch_for_qr(&input.progress_batch_id, &qr_payload)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "can_resume": batch.status == crate::core::production_map::OrderProgressBatchStatus::Paused,
        "batch": batch,
    })))
}

pub async fn production_map_progress_qr_report(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: ProgressQrLookupRequest = parse_json(&body)?;
    let qr_payload = if input.qr_payload.trim().is_empty() {
        input.progress_qr
    } else {
        input.qr_payload
    };
    let report = state
        .production_maps
        .progress_qr_report(&input.progress_batch_id, &qr_payload)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "scanned_batch": report.scanned_batch,
        "current_batch": report.current_batch,
        "is_stale": report.is_stale,
        "stale_reason": report.stale_reason,
        "order": report.order,
        "order_status": report.order_status,
        "queue_states": report.queue_states,
        "logs": report.logs,
        "progress_batches": report.progress_batches,
        "run_sessions": report.run_sessions,
        "active_sessions": report.active_sessions,
        "opened_by": report.opened_by,
    })))
}

#[derive(Default, serde::Deserialize)]
pub struct ProgressQrHistoryQuery {
    #[serde(default)]
    worker_ref: String,
    #[serde(default)]
    worker_display_name: String,
    #[serde(default)]
    limit: Option<usize>,
}

pub async fn production_map_progress_qr_history(
    State(state): State<AppState>,
    Query(query): Query<ProgressQrHistoryQuery>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueRead,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let (worker_refs, worker_display_name) = progress_history_scope(&principal, &query)?;
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let batches = state
        .production_maps
        .progress_batches_for_worker(&worker_refs, &worker_display_name, limit)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "batches": batches,
    })))
}

#[derive(serde::Deserialize)]
struct ProgressQrReprintRequest {
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
}

pub async fn production_map_progress_qr_reprint(
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
    let input: ProgressQrReprintRequest = parse_json(&body)?;
    let qr_payload = if input.qr_payload.trim().is_empty() {
        input.progress_qr.clone()
    } else {
        input.qr_payload.clone()
    };
    let batch = state
        .production_maps
        .progress_batch_for_qr(&input.progress_batch_id, &qr_payload)
        .await
        .map_err(production_map_error)?;
    if !principal_can_reprint_progress_batch(&principal, &batch) {
        return Err(forbidden());
    }
    let request = progress_reprint_request(&input, &batch);
    let print = match state.gscale.print_progress_label(request).await {
        Ok(response) => serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
        Err(error) => {
            tracing::warn!(
                error = %error,
                qr_payload = %batch.qr_payload,
                batch_id = %batch.batch_id,
                order_id = %batch.order_id,
                "progress qr reprint failed"
            );
            progress_print_failure_json(error)
        }
    };
    let ok = print
        .get("ok")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    Ok(json_response(serde_json::json!({
        "ok": ok,
        "batch": batch,
        "print": print,
    })))
}

fn progress_history_scope(
    principal: &Principal,
    query: &ProgressQrHistoryQuery,
) -> Result<(Vec<String>, String), AdminError> {
    if principal.role == PrincipalRole::Admin {
        let refs = query
            .worker_ref
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        let display_name = query.worker_display_name.trim().to_string();
        if refs.is_empty() && display_name.is_empty() {
            return Err(bad_request("worker_ref is required"));
        }
        return Ok((refs, display_name));
    }
    let principal_ref = principal.ref_.trim().to_string();
    if principal_ref.is_empty() {
        return Ok((Vec::new(), principal.display_name.trim().to_string()));
    }
    Ok((vec![principal_ref], String::new()))
}

fn principal_can_reprint_progress_batch(
    principal: &Principal,
    batch: &crate::core::production_map::OrderProgressBatch,
) -> bool {
    let principal_ref = principal.ref_.trim();
    principal.role == PrincipalRole::Admin
        || (!principal_ref.is_empty() && batch.worker_ref.trim() == principal_ref)
        || (principal_ref.is_empty()
            && !principal.display_name.trim().is_empty()
            && batch
                .worker_display_name
                .trim()
                .eq_ignore_ascii_case(principal.display_name.trim()))
}

fn progress_reprint_request(
    input: &ProgressQrReprintRequest,
    batch: &crate::core::production_map::OrderProgressBatch,
) -> ProgressLabelPrintRequest {
    ProgressLabelPrintRequest {
        driver_url: input.driver_url.clone(),
        qr_payload: batch.qr_payload.clone(),
        item_code: batch.label_item_code.clone(),
        item_name: batch.label_item_name.clone(),
        executor_name: batch.executor_name.clone(),
        printer: input.printer.clone(),
        print_mode: input.print_mode.clone(),
        gross_qty: batch.finished_goods_kg.unwrap_or(batch.produced_qty),
        progress_qty: batch.produced_qty,
        unit: "kg".to_string(),
        progress_unit: if batch.uom.trim().is_empty() {
            "m".to_string()
        } else {
            batch.uom.clone()
        },
        label_kind: String::new(),
        print_count: input.print_count,
    }
}
