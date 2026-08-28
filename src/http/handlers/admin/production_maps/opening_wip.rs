use super::*;

#[derive(Default, serde::Deserialize)]
pub struct OpeningWipHttpQuery {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    limit: Option<String>,
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
    let entry_apparatus = state
        .production_maps
        .resolve_canonical_apparatus_text(&details.intake.entry_apparatus)
        .await
        .map_err(production_map_error)?;
    let customer_name = state
        .production_maps
        .raw_map(&details.intake.order_id)
        .await
        .map_err(production_map_error)?
        .map(|map| map.customer_name)
        .unwrap_or_default();
    let quantity = details.batch.quantity.unwrap_or(0.0);
    let print_request = ProgressLabelPrintRequest {
        driver_url: input.driver_url.trim().to_string(),
        qr_payload: details.batch.qr_payload.clone(),
        item_code: details.batch.label_item_code.clone(),
        item_name: details.batch.label_item_name.clone(),
        apparatus: details.intake.entry_apparatus.clone(),
        apparatus_display_name: format!(
            "Opening WIP → {}",
            entry_apparatus.runtime.display.display_name.trim()
        ),
        customer_name: customer_name.trim().to_string(),
        executor_name: details.intake.actor.display_name.clone(),
        printer: input.printer.trim().to_string(),
        print_mode: input.print_mode.trim().to_string(),
        gross_qty: quantity,
        progress_qty: quantity,
        unit: details.batch.uom.clone(),
        progress_unit: details.batch.uom.clone(),
        label_kind: "opening_wip".to_string(),
        print_count: input.print_count.max(1),
        ..ProgressLabelPrintRequest::default()
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
