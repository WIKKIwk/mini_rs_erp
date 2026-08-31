use super::queue_actions::progress_print_failure_json;
use super::*;
use crate::core::production_map::ProgressBatchCorrectionInput;

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
    let input: ProgressQrLookupRequest = parse_json(&body)?;
    let qr_payload = if input.qr_payload.trim().is_empty() {
        input.progress_qr
    } else {
        input.qr_payload
    };
    if let Some(batch) = super::super::training::training_progress_batch_for_qr(
        &state,
        &principal,
        &input.progress_batch_id,
        &qr_payload,
    )
    .await
    .map_err(super::super::training::training_workspace_error)?
    {
        return Ok(json_response(serde_json::json!({
            "ok": true,
            "can_resume": matches!(
                batch.status,
                crate::core::production_map::OrderProgressBatchStatus::Paused
                    | crate::core::production_map::OrderProgressBatchStatus::RollDetached
            ),
            "batch": batch,
        })));
    }
    let batch = state
        .production_maps
        .progress_batch_for_qr(&input.progress_batch_id, &qr_payload)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "can_resume": matches!(
            batch.status,
            crate::core::production_map::OrderProgressBatchStatus::Paused
                | crate::core::production_map::OrderProgressBatchStatus::RollDetached
        ),
        "batch": batch,
    })))
}

pub async fn production_map_progress_qr_report(
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
            Capability::ApparatusQueueRead,
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
    if let Some(batch) = super::super::training::training_progress_batch_for_qr(
        &state,
        &principal,
        &input.progress_batch_id,
        &qr_payload,
    )
    .await
    .map_err(super::super::training::training_workspace_error)?
    {
        let mut progress_batches =
            super::super::training::training_progress_batches_for_order(&state, &batch.order_id)
                .await
                .map_err(super::super::training::training_workspace_error)?;
        if !progress_batches
            .iter()
            .any(|item| item.batch_id.eq_ignore_ascii_case(&batch.batch_id))
        {
            progress_batches.insert(0, batch.clone());
        }
        return Ok(json_response(serde_json::json!({
            "ok": true,
            "scanned_batch": batch.clone(),
            "current_batch": batch.clone(),
            "is_stale": false,
            "stale_reason": "",
            "queue_states": {},
            "logs": [],
            "corrections": [],
            "progress_batches": progress_batches,
            "run_sessions": [],
            "active_sessions": [],
        })));
    }
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
        "corrections": report.corrections,
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

pub async fn production_map_progress_batch_correct(
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
            Capability::ApparatusQueueRead,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: ProgressBatchCorrectionInput = parse_json(&body)?;
    let batch = state
        .production_maps
        .correct_progress_batch(input, &queue_action_actor(&principal))
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "batch": batch,
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
            Capability::ApparatusQueueRead,
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
    let batch = match super::super::training::training_progress_batch_for_qr(
        &state,
        &principal,
        &input.progress_batch_id,
        &qr_payload,
    )
    .await
    .map_err(super::super::training::training_workspace_error)?
    {
        Some(batch) => batch,
        None => state
            .production_maps
            .progress_batch_for_qr(&input.progress_batch_id, &qr_payload)
            .await
            .map_err(production_map_error)?,
    };
    if !principal_can_reprint_progress_batch(&principal, &batch) {
        return Err(forbidden());
    }
    let apparatus_display_name = state
        .production_maps
        .resolve_canonical_apparatus_text(&batch.apparatus)
        .await
        .map_err(production_map_error)?
        .runtime
        .display
        .display_name
        .clone();
    let item_name = match state.production_maps.raw_map(&batch.order_id).await {
        Ok(Some(order_map)) => crate::core::production_map::progress_label_item_name(
            &order_map,
            &batch.apparatus,
            batch.action,
        ),
        Ok(None) => batch.label_item_name.clone(),
        Err(error) => return Err(production_map_error(error)),
    };
    let request = progress_reprint_request(&input, &batch, &item_name, &apparatus_display_name);
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
        || (batch.order_id.trim().starts_with("training-")
            && batch
                .payload_json
                .get("training")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false))
}

fn progress_reprint_request(
    input: &ProgressQrReprintRequest,
    batch: &crate::core::production_map::OrderProgressBatch,
    item_name: &str,
    apparatus_display_name: &str,
) -> ProgressLabelPrintRequest {
    ProgressLabelPrintRequest {
        driver_url: input.driver_url.clone(),
        qr_payload: batch.qr_payload.clone(),
        item_code: batch.label_item_code.clone(),
        item_name: item_name.to_string(),
        apparatus: batch.apparatus.clone(),
        apparatus_display_name: apparatus_display_name.trim().to_string(),
        customer_name: batch
            .payload_json
            .get("customer_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        executor_name: batch.executor_name.clone(),
        printer: input.printer.clone(),
        print_mode: input.print_mode.clone(),
        gross_qty: batch
            .payload_json
            .get("gross_qty")
            .and_then(serde_json::Value::as_f64)
            .or(batch.finished_goods_kg)
            .unwrap_or(batch.produced_qty),
        tare_enabled: batch.bobina_kg.is_some_and(|value| value > 0.0),
        tare_kg: batch.bobina_kg.unwrap_or(0.0),
        progress_qty: batch.finished_goods_meter.unwrap_or(batch.produced_qty),
        unit: "kg".to_string(),
        progress_unit: if batch.uom.trim().is_empty() {
            "m".to_string()
        } else {
            batch.uom.clone()
        },
        label_kind: "progress".to_string(),
        print_count: input.print_count,
    }
}
