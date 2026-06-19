use super::*;
use crate::core::auth::models::PrincipalRole;
use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderTemplate, owner_key, validate_template,
};
use crate::core::gscale::models::{ProgressLabelPrintRequest, RawMaterialStockEntry};
use crate::core::production_map::{
    ApparatusMaterialRuleUpsert, ApparatusQueuePolicy, CompletionRequestDecision,
    ProductionMapBatchMoveRequest, ProductionMapDefinition, ProductionMapError,
    ProductionMapMoveRequest, ProductionMapRunRequest, QueueActionActor, QueueProgressInput,
    RawMaterialAssignment, RawMaterialAssignmentInput, queue_state,
};
use crate::core::werka::models::SupplierItem;
use crate::google_sheets::is_sheet_order_map;
use async_stream::stream;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_core::Stream;
use std::convert::Infallible;
use std::time::Duration;

pub async fn production_maps(
    State(state): State<AppState>,
    Query(query): Query<ProductionMapsQuery>,
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
        ],
    )
    .await?;
    if !matches!(method, Method::GET | Method::PUT) {
        return Err(method_not_allowed());
    }
    match method {
        Method::GET => {
            if !query.id.trim().is_empty() {
                let saved = state
                    .production_maps
                    .map(&query.id)
                    .await
                    .map_err(production_map_error)?
                    .ok_or_else(|| not_found("map_not_found"))?;
                return Ok(json_response(saved));
            }
            let maps = state
                .production_maps
                .maps()
                .await
                .map_err(|_| server_error("production maps fetch failed"))?;
            Ok(json_response(maps))
        }
        Method::PUT => {
            authorize_any_capability(
                &state,
                &headers,
                &[Capability::AdminAccess, Capability::ProductionMapManage],
            )
            .await?;
            let input: ProductionMapDefinition = parse_json(&body)?;
            match state.production_maps.upsert_map(input).await {
                Ok(saved) => Ok(json_response(saved)),
                Err(error) => Err(production_map_error(error)),
            }
        }
        _ => Err(method_not_allowed()),
    }
}

#[derive(Default, serde::Deserialize)]
pub struct ProductionMapsQuery {
    #[serde(default)]
    id: String,
}

#[derive(serde::Deserialize)]
struct ProductionMapSaveWithOrderRequest {
    map: ProductionMapDefinition,
    #[serde(default)]
    template: Option<CalculateOrderTemplate>,
}

/// Saves a production map and (optionally) its calculate order template in one
/// server-side operation, so the client never has to coordinate two writes.
pub async fn production_map_save_with_order(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::PUT {
        return Err(method_not_allowed());
    }
    let input: ProductionMapSaveWithOrderRequest = parse_json(&body)?;
    if let Some(template) = &input.template {
        validate_template(template).map_err(calculate_order_error)?;
    }
    let opens_quick_template_as_order = input
        .template
        .as_ref()
        .is_some_and(|template| is_quick_template_order_clone(&input.map, template));
    let owner_key = principal_owner_key(&principal);
    let map_id = input.map.id.trim().to_string();
    let previous = state
        .production_maps
        .raw_map(&map_id)
        .await
        .map_err(production_map_error)?;
    let saved_map = state
        .production_maps
        .upsert_map(input.map)
        .await
        .map_err(production_map_error)?;
    let mut integration_template = None;
    let saved_template = match input.template {
        Some(mut template) => {
            if opens_quick_template_as_order {
                integration_template = Some(template);
                None
            } else {
                template.source_map_id = saved_map.map.id.trim().to_string();
                match state
                    .calculate_orders
                    .upsert(&owner_key, template)
                    .await
                    .map_err(calculate_order_error)
                {
                    Ok(saved_template) => Some(saved_template),
                    Err(error) => {
                        if let Err(rollback_error) = state
                            .production_maps
                            .restore_map(previous.as_ref(), &map_id)
                            .await
                        {
                            tracing::error!(?rollback_error, "with-order map rollback failed");
                        }
                        return Err(error);
                    }
                }
            }
        }
        None => None,
    };
    if previous.is_none()
        && is_sheet_order_map(&saved_map.map)
        && let Some(template) = saved_template
            .clone()
            .or_else(|| integration_template.as_ref().cloned())
    {
        spawn_order_integrations(state.clone(), saved_map.map.clone(), template);
    }
    Ok(json_response(serde_json::json!({
        "ok": true,
        "saved": saved_map,
        "template": saved_template,
    })))
}

fn is_quick_template_order_clone(
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> bool {
    let source_map_id = template.source_map_id.trim();
    !source_map_id.is_empty() && source_map_id != map.id.trim() && is_sheet_order_map(map)
}

fn spawn_order_integrations(
    state: AppState,
    map: ProductionMapDefinition,
    template: CalculateOrderTemplate,
) {
    tokio::spawn(async move {
        if let Err(error) = state.order_sheets.append_order(&map, &template).await {
            tracing::warn!(?error, map_id = %map.id, "google sheets order append failed");
        }
        if let Err(error) = state.production_orders.save_order(&map, &template).await {
            tracing::warn!(?error, map_id = %map.id, "mini order save failed");
        }
    });
}

#[derive(serde::Deserialize)]
struct ApparatusSequencePutRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_ids: Vec<String>,
}

/// Apparatus order sequences are stored server-side so every device (admin
/// and worker) sees the same queue order.
pub async fn production_map_sequence(
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
        ],
    )
    .await?;
    match method {
        Method::GET => {
            let sequences = state
                .production_maps
                .apparatus_sequences()
                .await
                .map_err(production_map_error)?;
            let queue_states = state
                .production_maps
                .apparatus_queue_states()
                .await
                .map_err(production_map_error)?;
            let queue_policies = state
                .production_maps
                .apparatus_queue_policy_records()
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "sequences": sequences,
                "queue_states": queue_states,
                "queue_policies": queue_policies,
            })))
        }
        Method::PUT => {
            authorize_any_capability(
                &state,
                &headers,
                &[Capability::AdminAccess, Capability::ProductionMapManage],
            )
            .await?;
            let input: ApparatusSequencePutRequest = parse_json(&body)?;
            if input.apparatus.trim().is_empty() {
                return Err(bad_request("apparatus is required"));
            }
            state
                .production_maps
                .set_apparatus_sequence(&input.apparatus, input.order_ids)
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({"ok": true})))
        }
        _ => Err(method_not_allowed()),
    }
}

#[derive(serde::Deserialize)]
struct ApparatusQueuePolicyPutRequest {
    #[serde(default)]
    apparatus: String,
    policy: ApparatusQueuePolicy,
}

/// Apparatus queue policy controls whether a worker must follow the saved
/// sequence or can pick any ready order. Pechat stays strict in the service.
pub async fn production_map_queue_policies(
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
        ],
    )
    .await?;
    match method {
        Method::GET => {
            let policies = state
                .production_maps
                .apparatus_queue_policy_records()
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "policies": policies,
            })))
        }
        Method::PUT => {
            authorize_any_capability(
                &state,
                &headers,
                &[Capability::AdminAccess, Capability::ProductionMapManage],
            )
            .await?;
            let input: ApparatusQueuePolicyPutRequest = parse_json(&body)?;
            if input.apparatus.trim().is_empty() {
                return Err(bad_request("apparatus is required"));
            }
            let record = state
                .production_maps
                .set_apparatus_queue_policy(
                    &input.apparatus,
                    input.policy,
                    &queue_action_actor(&principal),
                )
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "policy": record,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

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
    action: queue_state::ApparatusQueueAction,
}

#[derive(serde::Deserialize)]
struct ProgressQrLookupRequest {
    #[serde(default)]
    progress_batch_id: String,
    #[serde(default)]
    qr_payload: String,
    #[serde(default)]
    progress_qr: String,
}

#[derive(serde::Deserialize)]
struct CompletionRequestDecisionRequest {
    #[serde(default)]
    event_id: String,
    #[serde(default)]
    decision: String,
}

/// Starts or completes the current actionable order on the operator's assigned
/// apparatus queue.
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
    let progress = QueueProgressInput {
        produced_qty: input.produced_qty.or(input.qty),
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
    };
    let _queue_action_guard = state.production_maps.queue_action_guard().await;
    if matches!(input.action, queue_state::ApparatusQueueAction::Complete)
        && progress.produced_qty.is_none()
        && input.gross_qty.is_none()
        && !input.completion_request_note.trim().is_empty()
    {
        let result = state
            .production_maps
            .request_completion_without_output(
                &input.apparatus,
                &input.order_id,
                &assigned_apparatus,
                queue_action_actor(&principal),
                &input.completion_request_note,
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
                gross_qty: input.gross_qty.unwrap_or(batch.produced_qty),
                progress_qty: batch.produced_qty,
                unit: "kg".to_string(),
                progress_unit: if batch.uom.trim().is_empty() {
                    "m".to_string()
                } else {
                    batch.uom.clone()
                },
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
        let response = state
            .gscale
            .print_progress_label(request)
            .await
            .map_err(gscale_progress_error)?;
        print = serde_json::to_value(response).unwrap_or(serde_json::Value::Null);
    }
    Ok(json_response(serde_json::json!({
        "ok": true,
        "states": result.states,
        "session": result.session,
        "progress_event": result.progress_event,
        "progress_batch": result.progress_batch,
        "print": print,
    })))
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

/// Configures which raw-material item groups are allowed for each apparatus.
pub async fn raw_material_rules(
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
            Capability::RawMaterialRuleManage,
        ],
    )
    .await?;
    match method {
        Method::GET => {
            require_capability(&state, &principal, Capability::RawMaterialRuleManage).await?;
            state
                .production_maps
                .apparatus_material_rules()
                .await
                .map(json_response)
                .map_err(production_map_error)
        }
        Method::PUT => {
            require_capability(&state, &principal, Capability::RawMaterialRuleManage).await?;
            let input: ApparatusMaterialRuleUpsert = parse_json(&body)?;
            state
                .production_maps
                .set_apparatus_material_rule(input)
                .await
                .map(json_response)
                .map_err(production_map_error)
        }
        _ => Err(method_not_allowed()),
    }
}

/// Assigns a printed raw-material QR to the order apparatus selected by rules.
pub async fn raw_material_assignments(
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
            Capability::RawMaterialAssign,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    match method {
        Method::GET => {
            let assignments = state
                .production_maps
                .raw_material_assignments()
                .await
                .map_err(production_map_error)?;
            Ok(json_response(
                raw_material_assignment_responses(&state, assignments).await,
            ))
        }
        Method::POST => {
            require_capability(&state, &principal, Capability::RawMaterialAssign).await?;
            let input: RawMaterialAssignmentInput = parse_json(&body)?;
            let (input, warehouse) = fill_raw_material_assignment_input(&state, input).await?;
            let assigned = state
                .production_maps
                .assign_raw_material_to_order(input, &queue_action_actor(&principal))
                .await
                .map_err(production_map_error)?;
            state
                .warehouse_events
                .notify_updated(&warehouse, "raw_material_assignment");
            Ok(json_response(
                raw_material_assignment_response(&state, assigned).await,
            ))
        }
        _ => Err(method_not_allowed()),
    }
}

async fn raw_material_assignment_responses(
    state: &AppState,
    assignments: Vec<RawMaterialAssignment>,
) -> Vec<serde_json::Value> {
    let mut response = Vec::with_capacity(assignments.len());
    for assignment in assignments {
        response.push(raw_material_assignment_response(state, assignment).await);
    }
    response
}

async fn raw_material_assignment_response(
    state: &AppState,
    assignment: RawMaterialAssignment,
) -> serde_json::Value {
    let stock = state
        .gscale
        .raw_material_stock_by_barcode(&assignment.barcode)
        .await
        .ok()
        .flatten();
    let mut value = serde_json::to_value(&assignment).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "stock_status".to_string(),
            serde_json::Value::String(
                stock
                    .as_ref()
                    .map(|entry| entry.status.clone())
                    .unwrap_or_default(),
            ),
        );
        object.insert(
            "reserved_order_id".to_string(),
            serde_json::Value::String(
                stock
                    .as_ref()
                    .map(|entry| entry.reserved_order_id.clone())
                    .unwrap_or_default(),
            ),
        );
        object.insert(
            "stock_warehouse".to_string(),
            serde_json::Value::String(stock.map(|entry| entry.warehouse).unwrap_or_default()),
        );
    }
    value
}

async fn raw_material_barcodes_for_order_apparatus(
    state: &AppState,
    order_id: &str,
    apparatus: &str,
) -> Result<Vec<String>, AdminError> {
    let assignments = state
        .production_maps
        .raw_material_assignments()
        .await
        .map_err(production_map_error)?;
    Ok(assignments
        .into_iter()
        .filter(|assignment| {
            assignment.order_id.trim() == order_id.trim()
                && queue_state::apparatus_titles_match(&assignment.apparatus, apparatus)
        })
        .map(|assignment| assignment.barcode.trim().to_string())
        .filter(|barcode| !barcode.is_empty())
        .collect())
}

fn raw_material_stock_status_error(error: crate::core::gscale::GscaleServiceError) -> AdminError {
    match error {
        crate::core::gscale::GscaleServiceError::InvalidInput(detail) => bad_request(detail),
        _ => server_error("raw material stock status update failed"),
    }
}

pub async fn raw_material_stock(
    State(state): State<AppState>,
    Query(query): Query<ItemQuery>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::CatalogItemRead,
            Capability::RawMaterialAssign,
        ],
    )
    .await?;
    require_capability(&state, &principal, Capability::CatalogItemRead).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let limit = optional_search_limit(query.limit.as_deref(), 200, 500);
    state
        .gscale
        .raw_material_stock(query.warehouse.as_deref().unwrap_or(""), limit)
        .await
        .map(json_response)
        .map_err(|_| server_error("raw material stock fetch failed"))
}

pub async fn raw_material_assignment_lookup(
    State(state): State<AppState>,
    Query(query): Query<RawMaterialAssignmentLookupQuery>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::RawMaterialAssign,
        ],
    )
    .await?;
    require_capability(&state, &principal, Capability::RawMaterialAssign).await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let detail = lookup_raw_material_detail(&state, &query.barcode).await?;
    Ok(json_response(detail))
}

#[derive(Default, serde::Deserialize)]
pub struct RawMaterialAssignmentLookupQuery {
    #[serde(default)]
    barcode: String,
}

#[derive(serde::Serialize)]
struct RawMaterialLookupResponse {
    barcode: String,
    warehouse: String,
    item_code: String,
    item_name: String,
    item_group: String,
    qty: f64,
    uom: String,
}

async fn fill_raw_material_assignment_input(
    state: &AppState,
    mut input: RawMaterialAssignmentInput,
) -> Result<(RawMaterialAssignmentInput, String), AdminError> {
    let barcode = input.barcode.trim();
    if barcode.is_empty() {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    }
    if !input.item_code.trim().is_empty() && !input.item_group.trim().is_empty() {
        let stock = state
            .gscale
            .raw_material_stock_by_barcode(barcode)
            .await
            .map_err(|_| production_map_error(ProductionMapError::RawMaterialInvalidInput))?
            .ok_or_else(|| production_map_error(ProductionMapError::RawMaterialInvalidInput))?;
        return Ok((input, stock.warehouse.trim().to_string()));
    }
    let (stock, item) = resolve_raw_material_stock_item(state, barcode).await?;
    let item_code = stock.item_code.trim().to_string();
    if item_code.is_empty() {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    }
    input.item_code = item_code;
    input.item_name = if input.item_name.trim().is_empty() {
        item.name.trim().to_string()
    } else {
        input.item_name.trim().to_string()
    };
    input.item_group = item.item_group.trim().to_string();
    Ok((input, stock.warehouse.trim().to_string()))
}

async fn lookup_raw_material_detail(
    state: &AppState,
    barcode: &str,
) -> Result<RawMaterialLookupResponse, AdminError> {
    let (stock, item) = resolve_raw_material_stock_item(state, barcode).await?;
    Ok(RawMaterialLookupResponse {
        barcode: stock.barcode.trim().to_string(),
        warehouse: stock.warehouse.trim().to_string(),
        item_code: stock.item_code.trim().to_string(),
        item_name: item.name.trim().to_string(),
        item_group: item.item_group.trim().to_string(),
        qty: stock.qty,
        uom: stock.uom.trim().to_string(),
    })
}

async fn resolve_raw_material_stock_item(
    state: &AppState,
    barcode: &str,
) -> Result<(RawMaterialStockEntry, SupplierItem), AdminError> {
    let barcode = barcode.trim();
    if barcode.is_empty() {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    }
    let stock = state
        .gscale
        .raw_material_stock_by_barcode(barcode)
        .await
        .map_err(|_| production_map_error(ProductionMapError::RawMaterialInvalidInput))?
        .ok_or_else(|| production_map_error(ProductionMapError::RawMaterialInvalidInput))?;
    let item_code = stock.item_code.trim().to_string();
    if item_code.is_empty() {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    }
    let items = state
        .admin
        .items_by_codes(std::slice::from_ref(&item_code))
        .await
        .map_err(|_| production_map_error(ProductionMapError::RawMaterialInvalidInput))?;
    let Some(item) = items
        .into_iter()
        .find(|item| item.code.trim().eq_ignore_ascii_case(&item_code))
    else {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    };
    Ok((stock, item))
}

/// Pushes production-map queue snapshots over SSE so operators see changes
/// instantly without polling.
pub async fn production_map_live(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    let include_completion_requests = matches!(principal.role, PrincipalRole::Admin);
    Ok(production_map_live_sse(
        state,
        queue_action_actor(&principal).ref_,
        include_completion_requests,
    )
    .into_response())
}

fn production_map_live_sse(
    state: AppState,
    actor_ref: String,
    include_completion_requests: bool,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let service = state.production_maps.clone();
    let mut rx = service.subscribe_live();
    let event_stream = stream! {
        let mut last_payload = String::new();
        loop {
            let snapshot = service.live_snapshot().await;
            let completed_orders = service
                .completed_queue_orders_for_actor(&actor_ref, 200)
                .await;
            let completion_requests = if include_completion_requests {
                service.completion_requests(200).await
            } else {
                Ok(Vec::new())
            };
            let completion_request_decisions = service
                .completion_request_decisions_for_actor(&actor_ref, 200)
                .await;
            match (snapshot, completed_orders, completion_requests, completion_request_decisions) {
                (Ok(snapshot), Ok(completed_orders), Ok(completion_requests), Ok(completion_request_decisions)) => {
                    let payload = serde_json::json!({
                        "ok": true,
                        "maps": snapshot.maps,
                        "sequences": snapshot.sequences,
                        "queue_states": snapshot.queue_states,
                        "queue_policies": snapshot.queue_policies,
                        "completed_orders": completed_orders,
                        "completion_requests": completion_requests,
                        "completion_request_decisions": completion_request_decisions,
                    });
                    if let Ok(json) = serde_json::to_string(&payload) {
                        if json != last_payload {
                            last_payload = json.clone();
                            yield Ok(Event::default().event("snapshot").data(json));
                        }
                    }
                }
                (Err(error), _, _, _)
                | (_, Err(error), _, _)
                | (_, _, Err(error), _)
                | (_, _, _, Err(error)) => {
                    yield Ok(Event::default().event("error").data(error.to_string()));
                }
            }

            match rx.recv().await {
                Ok(()) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(event_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

pub async fn production_map_completed_orders(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    let completed_orders = state
        .production_maps
        .completed_queue_orders_for_actor(&queue_action_actor(&principal).ref_, 200)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "completed_orders": completed_orders,
    })))
}

pub async fn production_map_completion_requests(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    let completion_requests = state
        .production_maps
        .completion_requests(200)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "completion_requests": completion_requests,
    })))
}

pub async fn production_map_completion_request_decision(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    let input: CompletionRequestDecisionRequest = parse_json(&body)?;
    let decision = CompletionRequestDecision::parse(&input.decision)
        .ok_or_else(|| bad_request("decision is required"))?;
    let result = state
        .production_maps
        .decide_completion_request(&input.event_id, decision, queue_action_actor(&principal))
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "states": result.states,
        "decision": result.decision,
    })))
}

pub async fn production_map_completion_request_decisions(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
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
    let decisions = state
        .production_maps
        .completion_request_decisions_for_actor(&queue_action_actor(&principal).ref_, 200)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "completion_request_decisions": decisions,
    })))
}

pub async fn production_map_closed_orders(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    let closed_orders = state
        .production_maps
        .fully_completed_orders(200)
        .await
        .map_err(production_map_error)?;
    Ok(json_response(serde_json::json!({
        "ok": true,
        "closed_orders": closed_orders,
    })))
}

fn calculate_order_error(error: CalculateOrderError) -> AdminError {
    match error {
        CalculateOrderError::InvalidInput(detail) => bad_request(detail),
        CalculateOrderError::StoreFailed => server_error("calculate order save failed"),
    }
}

/// Moves multiple orders between apparatus atomically.
pub async fn production_map_move_batch(
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
    let input: ProductionMapBatchMoveRequest = parse_json(&body)?;
    match state.production_maps.move_apparatus_batch(input).await {
        Ok(saved) => Ok(json_response(serde_json::json!({
            "ok": true,
            "saved": saved,
        }))),
        Err(error) => Err(production_map_error(error)),
    }
}

/// Moves an order between apparatus. Pechat compatibility is validated on the
/// server; the client only renders the outcome.
pub async fn production_map_move(
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
    let input: ProductionMapMoveRequest = parse_json(&body)?;
    match state.production_maps.move_apparatus(input).await {
        Ok(saved) => Ok(json_response(serde_json::json!({
            "ok": true,
            "saved": saved,
        }))),
        Err(error) => Err(production_map_error(error)),
    }
}

fn production_map_error(error: ProductionMapError) -> AdminError {
    match error {
        ProductionMapError::DuplicateOrderNumber => bad_request("duplicate_order_number"),
        ProductionMapError::OrderNumberImmutable => bad_request("order_number_immutable"),
        ProductionMapError::MoveNotAllowed => bad_request("move_not_allowed"),
        ProductionMapError::QueueActionNotAllowed => bad_request("queue_action_not_allowed"),
        ProductionMapError::PreviousStageNotCompleted => {
            bad_request("previous_stage_not_completed")
        }
        ProductionMapError::ApparatusNotAssigned => bad_request("apparatus_not_assigned"),
        ProductionMapError::LaminatsiyaRubberTooLarge => {
            bad_request("laminatsiya_rubber_too_large")
        }
        ProductionMapError::ApparatusQueuePolicyLocked => bad_request("queue_policy_locked"),
        ProductionMapError::RawMaterialInvalidInput => bad_request("raw_material_invalid_input"),
        ProductionMapError::RawMaterialGroupNotAllowed => {
            bad_request("raw_material_group_not_allowed")
        }
        ProductionMapError::RawMaterialGroupAmbiguous => {
            bad_request("raw_material_group_ambiguous")
        }
        ProductionMapError::RawMaterialAlreadyAssigned => {
            bad_request("raw_material_already_assigned")
        }
        ProductionMapError::RawMaterialAlreadyAssignedToOrder => {
            bad_request("raw_material_already_assigned_to_order")
        }
        ProductionMapError::RawMaterialAssignmentNotFound => {
            bad_request("raw_material_assignment_not_found")
        }
        ProductionMapError::RawMaterialScanRequired => bad_request("raw_material_scan_required"),
        ProductionMapError::RawMaterialMismatch => bad_request("raw_material_mismatch"),
        ProductionMapError::ProgressInputInvalid => bad_request("progress_input_invalid"),
        ProductionMapError::ProgressBatchNotFound => not_found("progress_batch_not_found"),
        ProductionMapError::ProgressBatchNotResumable => {
            bad_request("progress_batch_not_resumable")
        }
        ProductionMapError::MapNotFound => not_found("map_not_found"),
        ProductionMapError::StoreFailed => server_error("store failed"),
        other => bad_request(other.to_string()),
    }
}

fn gscale_progress_error(error: crate::core::gscale::GscaleServiceError) -> AdminError {
    match error {
        crate::core::gscale::GscaleServiceError::InvalidInput(detail) => bad_request(detail),
        crate::core::gscale::GscaleServiceError::NotConfigured(_) => {
            service_unavailable("scale_driver_not_configured")
        }
        crate::core::gscale::GscaleServiceError::PrintFailed { detail, .. } => {
            failed_dependency(detail)
        }
        crate::core::gscale::GscaleServiceError::EpcGenerationFailed
        | crate::core::gscale::GscaleServiceError::StoreWrite(_)
        | crate::core::gscale::GscaleServiceError::SubmitFailed(_) => {
            failed_dependency(error.to_string())
        }
    }
}

fn service_unavailable(error: impl Into<String>) -> AdminError {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(AdminErrorResponse {
            error: error.into(),
        }),
    )
}

fn failed_dependency(error: impl Into<String>) -> AdminError {
    (
        StatusCode::FAILED_DEPENDENCY,
        Json(AdminErrorResponse {
            error: error.into(),
        }),
    )
}

fn queue_action_actor(principal: &Principal) -> QueueActionActor {
    QueueActionActor {
        role: principal_role_code(&principal.role).to_string(),
        ref_: principal.ref_.trim().to_string(),
        display_name: principal.display_name.trim().to_string(),
    }
}

fn principal_role_code(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Admin => "admin",
    }
}

fn principal_owner_key(principal: &Principal) -> String {
    let role = principal_role_code(&principal.role);
    owner_key(role, &principal.ref_)
}

pub async fn production_map_run(
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
    let input: ProductionMapRunRequest = parse_json(&body)?;
    match state.production_maps.run_map(input).await {
        Ok(result) => Ok(json_response(result)),
        Err(error) => Err(bad_request(error.to_string())),
    }
}
