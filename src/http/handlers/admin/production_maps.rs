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

mod completion;
mod move_run;
mod queue_actions;
mod raw_materials;

pub use self::completion::{
    production_map_closed_orders, production_map_completed_orders,
    production_map_completion_request_decision, production_map_completion_request_decisions,
    production_map_completion_requests, production_map_live,
};
pub use self::move_run::{production_map_move, production_map_move_batch, production_map_run};
pub use self::queue_actions::{production_map_progress_qr_lookup, production_map_queue_action};
pub use self::raw_materials::{
    raw_material_assignment_lookup, raw_material_assignments, raw_material_rules,
    raw_material_stock,
};

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

fn calculate_order_error(error: CalculateOrderError) -> AdminError {
    match error {
        CalculateOrderError::InvalidInput(detail) => bad_request(detail),
        CalculateOrderError::StoreFailed => server_error("calculate order save failed"),
    }
}

/// Moves multiple orders between apparatus atomically.

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
