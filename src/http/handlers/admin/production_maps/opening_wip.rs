use super::queue_actions::resolve_queue_apparatus;
use super::*;
use crate::core::production_map::OpeningWipIntakeStatus;

#[derive(Default, serde::Deserialize)]
pub struct OpeningWipHttpQuery {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    limit: Option<String>,
    #[serde(default)]
    batch_id: String,
}

#[derive(Default, serde::Deserialize)]
pub struct OpeningWipPrintRequest {
    #[serde(default)]
    batch_id: String,
    #[serde(default)]
    qr_payload: String,
    #[serde(default)]
    driver_url: String,
    #[serde(default)]
    printer: String,
    #[serde(default)]
    print_mode: String,
    #[serde(default)]
    print_transport: String,
    #[serde(default)]
    print_count: u32,
}

#[derive(Default, serde::Deserialize)]
pub struct OpeningWipLookupRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    batch_id: String,
    #[serde(default)]
    qr_payload: String,
}

#[derive(Default, serde::Deserialize)]
pub struct OpeningWipQrReportRequest {
    #[serde(default)]
    qr_payload: String,
}

#[derive(Default, serde::Deserialize)]
pub struct OpeningWipDeleteRequest {
    #[serde(default)]
    batch_id: String,
}

pub async fn production_map_opening_wip(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<OpeningWipHttpQuery>,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    match method {
        Method::POST => {
            let input: OpeningWipCreateInput = parse_json(&body)?;
            let record = state
                .production_maps
                .create_opening_wip(input, queue_action_actor(&principal))
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({ "record": record })))
        }
        Method::GET => {
            let wip_status = if query.status.trim().is_empty()
                || query.status.trim().eq_ignore_ascii_case("all")
            {
                None
            } else {
                Some(
                    OpeningWipBatchStatus::parse(&query.status)
                        .ok_or_else(|| bad_request("opening_wip_status_invalid"))?,
                )
            };
            let records = state
                .production_maps
                .opening_wip_records(OpeningWipQuery {
                    order_id: query.order_id.trim().to_string(),
                    wip_status,
                    limit: positive_int(query.limit.as_deref(), 100).min(500),
                })
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({ "records": records })))
        }
        Method::DELETE => {
            if !state
                .admin
                .principal_has_capability(&principal, Capability::AdminAccess)
                .await
            {
                return Err(forbidden());
            }
            let batch_id = if query.batch_id.trim().is_empty() {
                parse_json::<OpeningWipDeleteRequest>(&body)?.batch_id
            } else {
                query.batch_id
            };
            let deleted = state
                .production_maps
                .delete_opening_wip_batch(&batch_id, queue_action_actor(&principal))
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "batch": deleted.batch,
                "intake": deleted.intake,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn production_map_opening_wip_print(
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
    let input: OpeningWipPrintRequest = parse_json(&body)?;
    let details = state
        .production_maps
        .opening_wip_batch(&input.batch_id, &input.qr_payload)
        .await
        .map_err(production_map_error)?;
    if details.intake.status != OpeningWipIntakeStatus::Confirmed
        || details.batch.wip_status == OpeningWipBatchStatus::Void
    {
        return Err(not_found("progress_batch_not_found"));
    }
    let source_apparatus_id = if details.intake.source_apparatus.trim().is_empty() {
        details.intake.entry_apparatus.as_str()
    } else {
        details.intake.source_apparatus.as_str()
    };
    let source_apparatus = state
        .production_maps
        .resolve_canonical_apparatus_text(source_apparatus_id)
        .await
        .map_err(production_map_error)?;
    let customer_name = state
        .production_maps
        .raw_map(&details.intake.order_id)
        .await
        .map_err(production_map_error)?
        .map(|map| map.customer_name)
        .unwrap_or_default();
    let print_request = ProgressLabelPrintRequest {
        driver_url: input.driver_url.trim().to_string(),
        qr_payload: details.batch.qr_payload.clone(),
        item_code: details.batch.label_item_code.clone(),
        item_name: details.batch.label_item_name.clone(),
        apparatus: source_apparatus_id.to_string(),
        apparatus_display_name: source_apparatus
            .runtime
            .display
            .display_name
            .trim()
            .to_string(),
        customer_name: customer_name.trim().to_string(),
        executor_name: details.intake.actor.display_name.clone(),
        printer: input.printer.trim().to_string(),
        print_mode: input.print_mode.trim().to_string(),
        gross_qty: details.batch.finished_goods_kg.unwrap_or(0.0),
        progress_qty: details.batch.finished_goods_meter.unwrap_or(0.0),
        unit: "kg".to_string(),
        progress_unit: "m".to_string(),
        tare_enabled: details.batch.bobina_kg.is_some(),
        tare_kg: details.batch.bobina_kg.unwrap_or(0.0),
        label_kind: "progress".to_string(),
        print_count: input.print_count.max(1),
    };
    let result = if input.print_transport.trim().eq_ignore_ascii_case("offline") {
        state.gscale.prepare_progress_label(print_request)
    } else {
        state.gscale.print_progress_label(print_request).await
    };
    let print = match result {
        Ok(response) => serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
        Err(error) => super::queue_actions::progress_print_failure_json(error),
    };
    Ok(json_response(serde_json::json!({
        "ok": print.get("ok").and_then(serde_json::Value::as_bool).unwrap_or(false),
        "batch": details.batch,
        "print": print,
    })))
}

pub async fn production_map_opening_wip_lookup(
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
    let input: OpeningWipLookupRequest = parse_json(&body)?;
    if input.apparatus.trim().is_empty() || input.order_id.trim().is_empty() {
        return Err(bad_request("opening_wip_invalid_input"));
    }
    let apparatus = resolve_queue_apparatus(&state, &input.apparatus).await?;
    let apparatus_id = apparatus.id.to_string();
    let can_view_all = state
        .admin
        .principal_has_capability(&principal, Capability::AdminAccess)
        .await
        || state
            .admin
            .principal_has_capability(&principal, Capability::ProductionMapManage)
            .await;
    if !can_view_all {
        let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
        if !queue_state::apparatus_matches_assigned(&apparatus_id, &assigned_apparatus) {
            return Err(bad_request("apparatus_not_assigned"));
        }
    }
    let order_map = state
        .production_maps
        .raw_map(input.order_id.trim())
        .await
        .map_err(production_map_error)?
        .ok_or_else(|| bad_request("opening_wip_qr_mismatch"))?;
    if input.qr_payload.trim().is_empty() && input.batch_id.trim().is_empty() {
        let records = state
            .production_maps
            .opening_wip_records(OpeningWipQuery {
                order_id: input.order_id.trim().to_string(),
                wip_status: Some(OpeningWipBatchStatus::Waiting),
                limit: 10_000,
            })
            .await
            .map_err(production_map_error)?;
        let batches = records
            .into_iter()
            .filter(|record| {
                record.intake.status == OpeningWipIntakeStatus::Confirmed
                    && record.intake.order_id.trim() == input.order_id.trim()
                    && crate::core::production_map::ProductionMapService::opening_wip_target_stage(
                        &order_map,
                        &record.intake,
                        &apparatus_id,
                        "",
                    )
                    .is_some()
            })
            .flat_map(|record| record.batches)
            .filter(|batch| {
                batch.wip_status == OpeningWipBatchStatus::Waiting
                    && batch.order_id.trim() == input.order_id.trim()
            })
            .collect::<Vec<_>>();
        return Ok(json_response(serde_json::json!({
            "ok": true,
            "batches": batches,
        })));
    }
    if input.qr_payload.trim().is_empty() {
        return Err(bad_request("opening_wip_invalid_input"));
    }
    let details = match state
        .production_maps
        .opening_wip_batch(&input.batch_id, &input.qr_payload)
        .await
    {
        Ok(details) => details,
        Err(ProductionMapError::ProgressBatchNotFound) => {
            return Err(bad_request("opening_wip_qr_mismatch"));
        }
        Err(error) => return Err(production_map_error(error)),
    };
    let batch_id_matches =
        input.batch_id.trim().is_empty() || details.batch.batch_id.trim() == input.batch_id.trim();
    if details.intake.status != OpeningWipIntakeStatus::Confirmed
        || details.batch.wip_status != OpeningWipBatchStatus::Waiting
        || details.intake.order_id.trim() != input.order_id.trim()
        || details.batch.order_id.trim() != input.order_id.trim()
        || crate::core::production_map::ProductionMapService::opening_wip_target_stage(
            &order_map,
            &details.intake,
            &apparatus_id,
            "",
        )
        .is_none()
        || !batch_id_matches
        || !details
            .batch
            .qr_payload
            .trim()
            .eq_ignore_ascii_case(input.qr_payload.trim())
    {
        return Err(bad_request("opening_wip_qr_mismatch"));
    }
    Ok(json_response(serde_json::json!({
        "ok": true,
        "batch": details.batch,
    })))
}

pub async fn production_map_opening_wip_qr_report(
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
            Capability::ApparatusQueueRead,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let input: OpeningWipQrReportRequest = parse_json(&body)?;
    if input.qr_payload.trim().is_empty() {
        return Err(bad_request("opening_wip_qr_not_found"));
    }
    let details = match state
        .production_maps
        .opening_wip_batch("", &input.qr_payload)
        .await
    {
        Ok(details) => details,
        Err(ProductionMapError::ProgressBatchNotFound) => {
            return Err(not_found("opening_wip_qr_not_found"));
        }
        Err(error) => return Err(production_map_error(error)),
    };
    if details.intake.status != OpeningWipIntakeStatus::Confirmed
        || details.batch.wip_status == OpeningWipBatchStatus::Void
    {
        return Err(not_found("opening_wip_qr_not_found"));
    }
    Ok(json_response(serde_json::json!({
        "ok": true,
        "record": {
            "intake": details.intake,
            "batches": [details.batch],
        },
    })))
}
