use super::*;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderTemplate, owner_key, validate_template,
};
use crate::core::formula::{CalculateRequest, LayerInput, calculate};
use crate::core::gscale::models::{ProgressLabelPrintRequest, RawMaterialStockEntry};
use crate::core::production_map::{
    ApparatusMaterialRuleUpsert, ApparatusQueuePolicy, CompletionRequestDecision,
    MaterialScanProgressAction, OrderProgressBatchWipStatus, ProductionMapBatchMoveRequest,
    ProductionMapDefinition, ProductionMapError, ProductionMapMoveRequest, ProductionMapRunRequest,
    QueueActionActor, QueueProgressInput, RawMaterialAssignment, RawMaterialAssignmentDeleteInput,
    RawMaterialAssignmentInput, RawMaterialStockTransition, RawMaterialStockTransitionKind,
    WipProgressBatchQuery, queue_state,
};
use crate::google_sheets::is_sheet_order_map;

mod completion;
mod helpers;
mod move_run;
mod progress_qr;
mod queue_actions;
mod raw_material_details;
mod raw_materials;
mod wip;

pub use self::completion::{
    production_map_closed_orders, production_map_completed_orders,
    production_map_completion_request_decision, production_map_completion_request_decisions,
    production_map_completion_requests, production_map_live,
};
use self::helpers::*;
pub use self::move_run::{production_map_move, production_map_move_batch, production_map_run};
pub use self::progress_qr::{
    production_map_progress_qr_history, production_map_progress_qr_lookup,
    production_map_progress_qr_report, production_map_progress_qr_reprint,
};
pub use self::queue_actions::production_map_queue_action;
pub use self::raw_materials::{
    raw_material_assignment_lookup, raw_material_assignments, raw_material_rules,
    raw_material_stock,
};
pub use self::wip::{production_map_finished_goods_receive, production_map_wip_batches};

pub async fn production_map_audit(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[Capability::AdminAccess, Capability::ProductionMapManage],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let report = state
        .production_maps
        .audit_production_workflow()
        .await
        .map_err(production_map_error)?;
    Ok(json_response(report))
}

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
    let mut input: ProductionMapSaveWithOrderRequest = parse_json(&body)?;
    if let Some(template) = &input.template {
        validate_template(template).map_err(calculate_order_error)?;
        if template.kg > 0.0 {
            apply_authoritative_calculation(&mut input.map, template)?;
        }
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
    if opens_quick_template_as_order && previous.is_some() {
        return Err(production_map_error(
            ProductionMapError::DuplicateOrderNumber,
        ));
    }
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
                template.source_map_id = template_source_map_id_for_save(&saved_map.map, &template);
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

fn template_source_map_id_for_save(
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> String {
    let source_map_id = template.source_map_id.trim();
    if source_map_id.is_empty() && !is_sheet_order_map(map) {
        map.id.trim().to_string()
    } else {
        source_map_id.to_string()
    }
}

fn apply_authoritative_calculation(
    map: &mut ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> Result<(), AdminError> {
    let response = calculate(CalculateRequest {
        order_number: if template.order_number.trim().is_empty() {
            None
        } else {
            Some(template.order_number.trim().to_string())
        },
        customer: if template.customer.trim().is_empty() {
            None
        } else {
            Some(template.customer.trim().to_string())
        },
        product: Some(template.product.trim().to_string()),
        status: if template.status.trim().is_empty() {
            None
        } else {
            Some(template.status.trim().to_string())
        },
        material_display: if template.material_display.trim().is_empty() {
            None
        } else {
            Some(template.material_display.trim().to_string())
        },
        color: if template.color.trim().is_empty() {
            None
        } else {
            Some(template.color.trim().to_string())
        },
        kg: Some(template.kg),
        frame_product_size_mm: Some(template.frame_product_size_mm),
        frame_count: Some(template.frame_count),
        edge_allowance_mm: Some(template.edge_allowance_mm),
        waste_percent: Some(template.waste_percent),
        roll_count: template.roll_count,
        first_layer: LayerInput::new(
            template.first_layer_material.trim(),
            template.first_layer_micron.trim(),
        ),
        second_layer: LayerInput::new(
            template.second_layer_material.trim(),
            template.second_layer_micron.trim(),
        ),
        third_layer: LayerInput::new(
            template.third_layer_material.trim(),
            template.third_layer_micron.trim(),
        ),
        note: if template.note.trim().is_empty() {
            None
        } else {
            Some(template.note.trim().to_string())
        },
        ..CalculateRequest::default()
    })
    .map_err(|error| bad_request(&error))?;

    let base_length = response
        .results
        .first()
        .map(|result| result.base_length)
        .ok_or_else(|| bad_request("calculate result is empty"))?;
    map.width_mm = Some(response.width_mm);
    map.order_kg = Some(response.kg);
    map.base_length = Some(base_length);
    map.roll_count = response.roll_count;
    Ok(())
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
                .effective_apparatus_sequences()
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
            let order_statuses = state
                .production_maps
                .order_status_details()
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "sequences": sequences,
                "queue_states": queue_states,
                "queue_policies": queue_policies,
                "order_statuses": order_statuses,
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

pub(super) async fn raw_material_barcodes_for_order_apparatus(
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
