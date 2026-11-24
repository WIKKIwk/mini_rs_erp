use super::queue_actions::progress_print_failure_json;
use super::*;
use crate::core::production_map::{
    OrderControlState, OrderProgressBatch, ProgressBatchCorrectionInput, WipProductionDay,
    WipProgressBatchQuery,
};

#[derive(serde::Deserialize)]
struct ProgressQrLookupRequest {
    #[serde(default)]
    progress_batch_id: String,
    #[serde(default)]
    progress_qr: String,
    #[serde(default)]
    qr_payload: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    require_active_order: bool,
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
            Capability::ProductionMapRead,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: ProgressQrLookupRequest = parse_json(&body)?;
    let qr_payload = effective_progress_qr_payload(&input.qr_payload, &input.progress_qr);
    let scoped = !input.apparatus.trim().is_empty() || !input.order_id.trim().is_empty();
    if scoped {
        if input.apparatus.trim().is_empty() || input.order_id.trim().is_empty() {
            return Err(bad_request("progress_input_invalid"));
        }
        let can_manage = state
            .admin
            .principal_has_admin_workspace_access(&principal)
            .await
            || state
                .admin
                .principal_has_capability(&principal, Capability::ProductionMapManage)
                .await
            || state
                .admin
                .principal_has_capability(&principal, Capability::ProductionMapRead)
                .await;
        if !can_manage {
            let assigned = state.principal_assigned_apparatus(&principal).await;
            if !queue_state::apparatus_matches_assigned(&input.apparatus, &assigned) {
                return Err(forbidden());
            }
        }
    }
    if let Some(batch) = super::super::training::training_progress_batch_for_qr(
        &state,
        &principal,
        &input.progress_batch_id,
        qr_payload,
    )
    .await
    .map_err(super::super::training::training_workspace_error)?
    {
        if scoped
            && (batch.order_id.trim() != input.order_id.trim()
                || batch.next_apparatus.trim() != input.apparatus.trim()
                || batch.wip_status != OrderProgressBatchWipStatus::Waiting)
        {
            return Err(bad_request("progress_batch_not_accepted"));
        }
        return Ok(json_response(serde_json::json!({
            "ok": true,
            "validated_apparatus": input.apparatus.trim(),
            "validated_order_id": input.order_id.trim(),
            "can_resume": batch.status.is_resumable(),
            "active_order_validated": input.require_active_order,
            "batch": batch,
        })));
    }
    // Identify the scanned roll before checking its route. A freeze applies
    // to the whole order, including waiting output from an earlier apparatus.
    let mut batch = state
        .production_maps
        .progress_batch_for_qr(&input.progress_batch_id, qr_payload)
        .await
        .map_err(production_map_error)?;
    if principal.role == PrincipalRole::RezkaMaster {
        // Validate the scanned roll before reporting order status. The caller's
        // target apparatus does not establish access to the scanned order.
        if scoped && input.order_id.trim() != batch.order_id.trim() {
            return Err(bad_request("progress_batch_not_accepted"));
        }
        super::super::rezka_master::require_order_scope(&state, &principal, &batch.order_id).await?;
    }
    if input.require_active_order {
        match state
            .production_maps
            .order_control_state(&batch.order_id)
            .await
            .map_err(production_map_error)?
            .state
        {
            OrderControlState::Frozen => {
                return Err(progress_qr_order_control_error(
                    &state,
                    &batch,
                    ProductionMapError::OrderFrozen,
                )
                .await);
            }
            OrderControlState::FreezeRequested => {
                return Err(progress_qr_order_control_error(
                    &state,
                    &batch,
                    ProductionMapError::OrderFreezeRequested,
                )
                .await);
            }
            OrderControlState::Active => {}
        }
    }
    if scoped {
        let result = state
            .production_maps
            .start_input_for_qr(
                &input.apparatus,
                &input.order_id,
                &input.progress_batch_id,
                qr_payload,
            )
            .await;
        batch = match result {
            Ok(batch) => batch,
            Err(
                error
                @ (ProductionMapError::OrderFrozen | ProductionMapError::OrderFreezeRequested),
            ) => {
                return Err(progress_qr_order_control_error(&state, &batch, error).await);
            }
            Err(error) => return Err(production_map_error(error)),
        };
    }
    let (input_route, input_route_error) =
        if batch.wip_status == OrderProgressBatchWipStatus::Waiting {
            match state
                .production_maps
                .progress_batch_input_route(&batch)
                .await
            {
                Ok(route) => (Some(route), None),
                Err(error) if !scoped => (
                    None,
                    Some(crate::core::production_map::wip_route::route_error_code(
                        &error,
                    )),
                ),
                Err(error) => return Err(production_map_error(error)),
            }
        } else {
            (None, None)
        };
    Ok(json_response(serde_json::json!({
        "ok": true,
        "validated_apparatus": input.apparatus.trim(),
        "validated_order_id": input.order_id.trim(),
        "can_resume": batch.status.is_resumable(),
        "active_order_validated": input.require_active_order,
        "batch": batch,
        "input_route": input_route,
        "input_route_error": input_route_error,
    })))
}

async fn progress_qr_order_control_error(
    state: &AppState,
    batch: &OrderProgressBatch,
    error: ProductionMapError,
) -> AdminError {
    let mut response = production_map_error(error);
    // Keep the freeze rejection even if its presentation-only title read fails.
    let title = state
        .production_maps
        .raw_map(&batch.order_id)
        .await
        .ok()
        .flatten()
        .map(|map| map.title.trim().to_string())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| {
            batch
                .label_item_name
                .split(", apparat:")
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        });
    if !title.is_empty() {
        response.1.0.order_title = Some(title);
    }
    response
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
            Capability::ProductionMapRead,
            Capability::ApparatusQueueRead,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: ProgressQrLookupRequest = parse_json(&body)?;
    let qr_payload = effective_progress_qr_payload(&input.qr_payload, &input.progress_qr);
    if let Some(batch) = super::super::training::training_progress_batch_for_qr(
        &state,
        &principal,
        &input.progress_batch_id,
        qr_payload,
    )
    .await
    .map_err(super::super::training::training_workspace_error)?
    {
        return Ok(json_response(serde_json::json!({
            "ok": true,
            "scanned_batch": batch.clone(),
            "current_batch": batch.clone(),
            "current_batches": [batch.clone()],
            "history_scope": "batch_lineage",
            "lineage_complete": false,
            "lineage_edges": [],
            "session_resources": [],
            "is_stale": false,
            "stale_reason": "",
            "queue_states": {},
            "logs": [],
            "corrections": [],
            "progress_batches": [batch],
            "run_sessions": [],
            "active_sessions": [],
        })));
    }
    let report = state
        .production_maps
        .progress_qr_report(&input.progress_batch_id, qr_payload)
        .await
        .map_err(production_map_error)?;
    if principal.role == PrincipalRole::RezkaMaster {
        super::super::rezka_master::require_order_scope(&state, &principal, &report.scanned_batch.order_id).await?;
    }
    Ok(json_response(serde_json::json!({
        "ok": true,
        "scanned_batch": report.scanned_batch,
        "current_batch": report.current_batch,
        "current_batches": report.current_batches,
        "history_scope": report.history_scope,
        "lineage_complete": report.lineage_complete,
        "lineage_edges": report.lineage_edges,
        "session_resources": report.session_resources,
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
    #[serde(default)]
    rezka_team: bool,
    from_unix: Option<i64>,
    until_unix: Option<i64>,
    #[serde(default)]
    before_batch_id: String,
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
            Capability::ProductionMapRead,
            Capability::ApparatusQueueRead,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    if query.rezka_team {
        return rezka_team_daily_history(&state, &principal, query, limit).await;
    }
    let (worker_refs, worker_display_name) = progress_history_scope(&principal, &query)?;
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

async fn rezka_team_daily_history(
    state: &AppState,
    principal: &Principal,
    query: ProgressQrHistoryQuery,
    limit: usize,
) -> Result<Response, AdminError> {
    if principal.role != PrincipalRole::RezkaMaster || principal.ref_.trim().is_empty() {
        return Err(forbidden());
    }
    let (Some(from_unix), Some(until_unix)) = (query.from_unix, query.until_unix) else {
        return Err(bad_request("invalid daily work date"));
    };
    if from_unix <= 0
        || !(1..=26 * 3600).contains(&until_unix.saturating_sub(from_unix))
        || until_unix > 253_402_300_799
        || query.before_batch_id.len() > 4096
    {
        return Err(bad_request("invalid daily work range or cursor"));
    }
    let producer_apparatus_ids = state.principal_assigned_apparatus(principal).await;
    let mut batches = if producer_apparatus_ids.is_empty() {
        Vec::new()
    } else {
        let mut store_query = WipProgressBatchQuery::new("", "", "", None, true, "", limit + 1);
        store_query.production_day = Some(WipProductionDay {
            producer_apparatus_ids,
            from_unix,
            until_unix,
        });
        store_query.before_batch_id = query.before_batch_id;
        state.production_maps.wip_progress_batches(store_query).await.map_err(production_map_error)?
    };
    let has_more = batches.len() > limit;
    batches.truncate(limit);
    let next_before_batch_id = has_more.then(|| batches.last().map(|batch| &batch.batch_id)).flatten();
    Ok(json_response(serde_json::json!({
        "ok": true,
        "batches": batches,
        "next_before_batch_id": next_before_batch_id,
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
    automatic_rezka_labels: bool,
    #[serde(default)]
    print_transport: String,
}

include!("progress_qr_parts/production_map_progress_qr_reprint.rs");
