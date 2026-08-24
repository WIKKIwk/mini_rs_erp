use super::*;

use crate::core::gscale::GscaleServiceError;

#[derive(Default, serde::Deserialize)]
pub struct PaddonsQuery {
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Default, serde::Deserialize)]
struct PaddonCreateRequest {
    #[serde(default)]
    location: String,
    #[serde(default)]
    note: String,
}

#[derive(Default, serde::Deserialize)]
struct PaddonItemRequest {
    #[serde(default)]
    code: String,
    #[serde(default)]
    progress_batch_id: String,
    #[serde(default)]
    qr_payload: String,
}

#[derive(Default, serde::Deserialize)]
struct PaddonItemsRequest {
    #[serde(default)]
    code: String,
    #[serde(default)]
    progress_batch_ids: Vec<String>,
}

#[derive(Default, serde::Deserialize)]
struct PaddonQrPrintRequest {
    #[serde(default)]
    code: String,
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

pub async fn production_map_paddons(
    State(state): State<AppState>,
    Query(query): Query<PaddonsQuery>,
    method: Method,
    headers: HeaderMap,
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
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let paddons = state
        .production_maps
        .paddons(query.limit.unwrap_or(50))
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddons": paddons,
    })))
}

pub async fn production_map_paddon_detail(
    State(state): State<AppState>,
    Query(query): Query<PaddonCodeQuery>,
    method: Method,
    headers: HeaderMap,
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
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let snapshot = state
        .production_maps
        .paddon_snapshot(&query.code)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "available_items": snapshot.available_items,
    })))
}

pub async fn production_map_paddon_qr_report(
    State(state): State<AppState>,
    Query(query): Query<PaddonCodeQuery>,
    method: Method,
    headers: HeaderMap,
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
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let snapshot = state
        .production_maps
        .paddon_scan_snapshot(&query.code)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "qr_payload": query.code.trim(),
    })))
}

pub async fn production_map_paddon_qr_print(
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
    let input: PaddonQrPrintRequest = parse_json(&body)?;
    let paddon = state
        .production_maps
        .paddon_summary(&input.code)
        .await
        .map_err(production_map_error)?;
    let code = paddon.code.clone();
    let print_request = ProgressLabelPrintRequest {
        driver_url: input.driver_url,
        qr_payload: code.clone(),
        item_code: code.clone(),
        item_name: format!("Paddon {code}"),
        apparatus: String::new(),
        apparatus_display_name: String::new(),
        customer_name: String::new(),
        executor_name: principal.display_name.trim().to_string(),
        printer: input.printer,
        print_mode: input.print_mode,
        label_kind: "paddon_code".to_string(),
        gross_qty: 1.0,
        tare_enabled: false,
        tare_kg: 0.0,
        progress_qty: 1.0,
        unit: "dona".to_string(),
        progress_unit: "dona".to_string(),
        print_count: input.print_count,
    };
    let print = if input.print_transport.trim().eq_ignore_ascii_case("offline") {
        state
            .gscale
            .prepare_progress_label(print_request)
            .map_err(paddon_print_error)?
    } else {
        state
            .gscale
            .print_progress_label(print_request)
            .await
            .map_err(paddon_print_error)?
    };
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": paddon,
        "qr_payload": code,
        "print": print,
    })))
}

fn paddon_print_error(error: GscaleServiceError) -> AdminError {
    match error {
        GscaleServiceError::InvalidInput(detail) => bad_request(detail),
        GscaleServiceError::NotConfigured(detail) => server_error(detail),
        GscaleServiceError::EpcGenerationFailed => server_error("epc_generation_failed"),
        GscaleServiceError::StoreWrite(detail) => server_error(detail),
        GscaleServiceError::PrintFailed { detail, .. } => server_error(detail),
        GscaleServiceError::SubmitFailed(detail) => server_error(detail),
    }
}

#[derive(Default, serde::Deserialize)]
pub struct PaddonCodeQuery {
    #[serde(default)]
    code: String,
}

pub async fn production_map_paddon_create(
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
    let input: PaddonCreateRequest = parse_json(&body)?;
    let paddon = state
        .production_maps
        .create_paddon(
            &input.location,
            &input.note,
            &queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": paddon,
    })))
}

pub async fn production_map_paddon_item_add(
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
    let input: PaddonItemRequest = parse_json(&body)?;
    let progress_batch_id = resolve_progress_batch_id(&state, &input).await?;
    let snapshot = state
        .production_maps
        .add_paddon_item(
            &input.code,
            &progress_batch_id,
            &queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "available_items": snapshot.available_items,
    })))
}

pub async fn production_map_paddon_items_add(
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
    let input: PaddonItemsRequest = parse_json(&body)?;
    let snapshot = state
        .production_maps
        .add_paddon_items(
            &input.code,
            &input.progress_batch_ids,
            &queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "available_items": snapshot.available_items,
    })))
}

pub async fn production_map_paddon_item_remove(
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
    let input: PaddonItemRequest = parse_json(&body)?;
    let progress_batch_id = input.progress_batch_id.trim();
    if progress_batch_id.is_empty() {
        return Err(bad_request("progress_batch_id_required"));
    }
    let snapshot = state
        .production_maps
        .remove_paddon_item(
            &input.code,
            progress_batch_id,
            &queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "available_items": snapshot.available_items,
    })))
}

pub async fn production_map_paddon_items_remove(
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
    let input: PaddonItemsRequest = parse_json(&body)?;
    let snapshot = state
        .production_maps
        .remove_paddon_items(
            &input.code,
            &input.progress_batch_ids,
            &queue_action_actor(&principal),
        )
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "paddon": snapshot.paddon,
        "items": snapshot.items,
        "available_items": snapshot.available_items,
    })))
}

async fn resolve_progress_batch_id(
    state: &AppState,
    input: &PaddonItemRequest,
) -> Result<String, AdminError> {
    let progress_batch_id = input.progress_batch_id.trim();
    if !progress_batch_id.is_empty() {
        return Ok(progress_batch_id.to_string());
    }
    let qr_payload = input.qr_payload.trim();
    if qr_payload.is_empty() {
        return Err(bad_request("progress_qr_required"));
    }
    let batch = state
        .production_maps
        .progress_batch_for_qr("", qr_payload)
        .await
        .map_err(production_map_error)?;
    Ok(batch.batch_id)
}
