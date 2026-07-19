use super::queue_actions::progress_print_failure_json;
use super::*;

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
    #[serde(default)]
    print_transport: String,
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
    let print_result = if input.print_transport.trim().eq_ignore_ascii_case("offline") {
        state.gscale.prepare_progress_label(request)
    } else {
        state.gscale.print_progress_label(request).await
    };
    let print = match print_result {
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
        if refs.is_empty() {
            return Err(bad_request("worker_ref is required"));
        }
        return Ok((refs, String::new()));
    }
    let principal_ref = principal.ref_.trim().to_string();
    if principal_ref.is_empty() {
        return Err(forbidden());
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
