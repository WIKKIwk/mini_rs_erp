use super::raw_material_details::{
    apparatus_id_matches_text, apparatus_id_matches_text_value, assigned_apparatus_contains,
    fill_raw_material_assignment_input, item_group_path, lookup_raw_material_detail,
    raw_material_rulon_match_metrics, require_material_item_group_scope,
    require_material_warehouse_scope, resolve_raw_material_stock_item, roll_width_allowance_mm,
    rulon_width_bounds, rulon_width_restriction_applies, validate_rulon_size_for_apparatus_map,
};
use super::*;
use crate::core::apparatus_standard::{MaterialExecutionPolicy, ToolingExecutionPolicy};
use crate::core::gscale::models::RawMaterialStockEntry;
use crate::core::inventory_movements::RawMaterialStatePlacement;
use crate::core::werka::models::SupplierItem;
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, RawMaterialEventQuery, RawMaterialEventScope,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) async fn validate_receipt_order_assignment(
    state: &AppState,
    principal: &Principal,
    order_id: &str,
    apparatus: &str,
    item_code: &str,
    warehouse: &str,
    width_mm: Option<f64>,
    micron: Option<f64>,
) -> Result<Option<serde_json::Value>, AdminError> {
    if order_id.trim().is_empty() { return Ok(None); }
    if !matches!(
        principal.role,
        PrincipalRole::TayyorlovMasteri | PrincipalRole::MaterialTaminotchi
    ) {
        return Err(forbidden());
    }
    require_capability(state, principal, Capability::RawMaterialAssign).await?;
    let destination = state.warehouses.warehouse(warehouse).await.map_err(warehouse_error)?;
    if !destination.is_some_and(|warehouse| !warehouse.is_group) {
        return Err(bad_request("warehouse_not_found"));
    }
    require_tayyorlov_order_scope(state, principal, order_id).await?;
    if !state.production_maps.raw_material_assignment_orders().await.map_err(production_map_error)?
        .iter().any(|saved| saved.map.id == order_id.trim()) {
        return Err(bad_request("raw_material_order_not_active"));
    }
    let item = state.admin.items_by_codes(&[item_code.trim().to_string()]).await
        .map_err(|_| server_error("material item lookup failed"))?
        .into_iter().next().ok_or_else(|| bad_request("material_item_not_found"))?;
    let stock = RawMaterialStockEntry {
        warehouse: warehouse.trim().into(), item_code: item.code.clone(),
        item_name: item.name.clone(), status: "available".into(), qty: 1.0,
        width_mm, micron, ..Default::default()
    };
    let (input, _) = super::raw_material_details::fill_raw_material_assignment_for_stock(
        state, principal, RawMaterialAssignmentInput {
            order_id: order_id.trim().into(), apparatus: apparatus.trim().into(), ..Default::default()
        }, stock, item,
    ).await?;
    let actor = queue_action_actor(principal);
    Ok(Some(serde_json::json!({
        "order_id":input.order_id,"apparatus":input.apparatus,"apparatus_id":input.apparatus,
        "barcode":"","item_code":input.item_code,"item_name":input.item_name,"item_group":input.item_group,
        "assigned_by_role":actor.role,"assigned_by_ref":actor.ref_,
        "assigned_by_display_name":actor.display_name,
        "assigned_at":time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
    })))
}

#[derive(Debug, serde::Deserialize)]
pub struct RawMaterialStartRequirementsQuery {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    material_barcodes: String,
}

impl RawMaterialStartRequirementsQuery {
    pub(super) fn for_bootstrap(order_id: &str, apparatus: &str, material_barcodes: &str) -> Self {
        Self {
            order_id: order_id.to_string(),
            apparatus: apparatus.to_string(),
            material_barcodes: material_barcodes.to_string(),
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct RawMaterialAssignmentsQuery {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    barcode: String,
}

impl RawMaterialAssignmentsQuery {
    pub(super) fn for_bootstrap(order_id: &str, apparatus: &str) -> Self {
        Self {
            order_id: order_id.to_string(),
            apparatus: apparatus.to_string(),
            ..Default::default()
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct RawMaterialAssignmentCandidateResponse {
    barcode: String,
    warehouse: String,
    item_code: String,
    item_name: String,
    item_group: String,
    qty: f64,
    uom: String,
    apparatus_options: Vec<String>,
    order_width_mm: Option<f64>,
    roll_width_mm: Option<f64>,
    leftover_width_mm: Option<f64>,
    match_type: String,
}

#[derive(Debug, serde::Serialize)]
struct RawMaterialAssignmentOrderCandidateResponse {
    order: crate::core::production_map::ProductionMapSaved,
    apparatus_options: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct RawMaterialAssignmentDiagnosticResponse {
    barcode: String,
    compatible: bool,
    reason: String,
    item_code: String,
    item_name: String,
    item_group: String,
    warehouse: String,
    stock_status: String,
    reserved_order_id: String,
    apparatus_options: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    order_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apparatus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    order_width_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    roll_width_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    minimum_width_mm: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    maximum_width_mm: Option<f64>,
}

impl RawMaterialAssignmentDiagnosticResponse {
    fn from_stock(stock: &RawMaterialStockEntry, item: &SupplierItem) -> Self {
        Self {
            barcode: stock.barcode.trim().to_string(),
            compatible: false,
            reason: "no_compatible_active_order".to_string(),
            item_code: stock.item_code.trim().to_string(),
            item_name: item.name.trim().to_string(),
            item_group: item.item_group.trim().to_string(),
            warehouse: stock.warehouse.trim().to_string(),
            stock_status: stock.status.trim().to_string(),
            reserved_order_id: stock.reserved_order_id.trim().to_string(),
            apparatus_options: Vec::new(),
            order_id: None,
            order_title: None,
            apparatus: None,
            order_width_mm: None,
            roll_width_mm: None,
            minimum_width_mm: None,
            maximum_width_mm: None,
        }
    }
}

pub async fn raw_material_start_requirements(
    State(state): State<AppState>,
    Query(query): Query<RawMaterialStartRequirementsQuery>,
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
    if query.order_id.trim().is_empty() || query.apparatus.trim().is_empty() {
        return Err(bad_request("apparatus and order_id are required"));
    }
    if principal.role == PrincipalRole::RezkaMaster {
        super::super::rezka_master::require_order_scope(&state, &principal, &query.order_id).await?;
        if !state.principal_assigned_apparatus(&principal).await.contains(&query.apparatus) {
            return Err(forbidden());
        }
    }
    if let Some(training_requirements) =
        super::super::training::training_raw_material_start_requirements(
            &state,
            &principal,
            &query.order_id,
            &query.apparatus,
            &query.material_barcodes,
        )
        .await
        .map_err(super::super::training::training_workspace_error)?
    {
        return Ok(json_response(training_requirements));
    }
    let staged_barcodes =
        raw_material_state_barcodes_for_order_apparatus(&state, &query.order_id, &query.apparatus)
            .await?;
    let usable_barcodes = raw_material_usable_barcodes(&state, &query.order_id, &query.apparatus).await?;
    let requirements = state
        .production_maps
        .raw_material_start_requirements_for_stock(
            &query.apparatus,
            &query.order_id,
            &staged_barcodes,
            &query.material_barcodes,
            Some(&usable_barcodes),
        )
        .await
        .map_err(production_map_error)?;
    let eligible_barcodes = requirements
        .eligible_barcodes
        .iter()
        .map(|barcode| barcode.trim().to_ascii_uppercase())
        .collect::<BTreeSet<_>>();
    let mut assignments = state
        .production_maps
        .raw_material_assignments()
        .await
        .map_err(production_map_error)?;
    assignments.retain(|assignment| assignment.order_id.trim() == query.order_id.trim());
    let mut start_assignments = assignments
        .iter()
        .filter(|assignment| {
            apparatus_id_matches_text(&assignment.apparatus_id, &query.apparatus)
                && eligible_barcodes.contains(&assignment.barcode.trim().to_ascii_uppercase())
        })
        .cloned()
        .collect::<Vec<_>>();
    sort_raw_material_assignments(&mut assignments);
    sort_raw_material_assignments(&mut start_assignments);
    let assignments = raw_material_assignment_responses(&state, assignments).await;
    let start_assignments = raw_material_assignment_responses(&state, start_assignments).await;
    Ok(json_response(serde_json::json!({
        "policy": requirements.policy,
        "requires_material": requirements.requires_material,
        "material_scan_required": requirements.material_scan_required,
        "requirement_groups": requirements.requirement_groups,
        "assigned_barcodes": requirements.assigned_barcodes,
        "staged_barcodes": requirements.staged_barcodes,
        "eligible_barcodes": requirements.eligible_barcodes,
        "required_scan_count": requirements.required_scan_count,
        "matched_scan_count": requirements.matched_scan_count,
        "assignments_satisfied": requirements.assignments_satisfied,
        "scan_satisfied": requirements.scan_satisfied,
        "assignments": assignments,
        "start_assignments": start_assignments,
    })))
}

pub(super) async fn raw_material_state_barcodes_for_order_apparatus(
    state: &AppState,
    order_id: &str,
    apparatus: &str,
) -> Result<Vec<String>, AdminError> {
    let assignment_barcodes = state.production_maps.raw_material_assignments_for_order(order_id)
        .await.map_err(production_map_error)?.into_iter()
        .filter(|assignment| apparatus_id_matches_text(&assignment.apparatus_id, apparatus))
        .map(|assignment| assignment.barcode).collect::<Vec<_>>();
    let placements = state
        .inventory_movements
        .raw_material_state_placements(&assignment_barcodes)
        .await
        .map_err(|_| server_error("raw material state placements failed"))?;
    Ok(placements
        .into_iter()
        .filter(|placement| state_placement_matches_apparatus(placement, apparatus))
        .map(|placement| placement.barcode)
        .collect())
}

pub(super) async fn raw_material_usable_barcodes(
    state: &AppState,
    order_id: &str,
    apparatus: &str,
) -> Result<Vec<String>, AdminError> {
    let assignments = state
        .production_maps
        .raw_material_assignments_for_order(order_id)
        .await
        .map_err(production_map_error)?
        .into_iter()
        .filter(|assignment| {
            assignment.order_id.trim() == order_id.trim()
                && apparatus_id_matches_text(&assignment.apparatus_id, apparatus)
        })
        .collect::<Vec<_>>();
    let mut usable = Vec::new();
    let mut transferred = None;
    for assignment in assignments {
        let stock = state.gscale.raw_material_stock_by_barcode(&assignment.barcode)
            .await.map_err(|_| server_error("raw material stock lookup failed"))?;
        let status = raw_material_execution_status(state, &assignment, stock.as_ref()).await?;
        if status == "in_use" && transferred.is_none() {
            transferred = Some(state.production_maps.in_use_print_transfer_materials(order_id, apparatus)
                .await.map_err(production_map_error)?);
        }
        if status == "compatible" || (status == "in_use" && transferred.as_ref().is_some_and(
            |barcodes| barcodes.iter().any(|barcode| barcode.eq_ignore_ascii_case(&assignment.barcode)),
        )) {
            usable.push(assignment.barcode);
        }
    }
    Ok(usable)
}

pub(super) async fn raw_material_execution_status(
    state: &AppState,
    assignment: &RawMaterialAssignment,
    stock: Option<&RawMaterialStockEntry>,
) -> Result<&'static str, AdminError> {
    let Some(stock) = stock else { return Ok("unavailable"); };
    if stock.reserved_order_id == assignment.order_id && stock.status == "in_use" {
        return Ok("in_use");
    }
    if stock.status == "consumed" { return Ok("consumed"); }
    if stock.status != "available" || !stock.reserved_order_id.is_empty() || stock.qty <= 0.0 {
        return Ok("unavailable");
    }
    let (_, item) = resolve_raw_material_stock_item(state, &assignment.barcode).await?;
    let groups = state.admin.item_group_tree().await
        .map_err(|_| server_error("item group tree fetch failed"))?;
    let group_path = item_group_path(&groups, &item.item_group);
    if !group_path.iter().any(|group| group.eq_ignore_ascii_case("Rulon")) {
        return Ok("compatible");
    }
    let Some(allowance) = roll_width_allowance_mm(state, assignment.apparatus_id.as_str()).await? else {
        return Ok("compatible");
    };
    let map = state.production_maps.raw_map(&assignment.order_id).await
        .map_err(production_map_error)?.ok_or_else(|| production_map_error(ProductionMapError::MapNotFound))?;
    Ok(roll_execution_status(map.width_mm, super::raw_material_details::roll_width_mm(stock, &item), allowance))
}

fn roll_execution_status(order_width: Option<f64>, roll_width: Option<f64>, allowance: f64) -> &'static str {
    match (order_width, roll_width) {
        (Some(order), Some(roll)) if order.is_finite() && roll.is_finite() && order > 0.0 && roll > 0.0 => {
            let (minimum, maximum) = rulon_width_bounds(order, allowance);
            if roll > maximum + f64::EPSILON { "needs_cutting" }
            else if roll + f64::EPSILON < minimum { "width_mismatch" }
            else { "compatible" }
        }
        _ => "dimensions_missing",
    }
}

fn state_placement_matches_apparatus(
    placement: &RawMaterialStatePlacement,
    apparatus: &str,
) -> bool {
    placement
        .apparatus_ids
        .iter()
        .any(|candidate| apparatus_id_matches_text_value(candidate, apparatus))
}

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
            let rules = state
                .apparatus
                .list_runtime_configurations()
                .await
                .map_err(canonical_apparatus_error)?
                .into_iter()
                .map(|configuration| configuration.material)
                .collect::<Vec<_>>();
            Ok(json_response(rules))
        }
        Method::PUT => {
            require_capability(&state, &principal, Capability::RawMaterialRuleManage).await?;
            let input: CanonicalMaterialPatchRequest = parse_json(&body)?;
            let current = state
                .apparatus
                .current_configuration(&input.apparatus_id)
                .await
                .map_err(canonical_apparatus_error)?
                .ok_or_else(|| not_found("apparatus_not_found"))?;
            let committed = state
                .apparatus
                .patch(
                    input.apparatus_id,
                    input.expected_revision,
                    CanonicalApparatusPatch {
                        policies: Some(ApparatusOperationalPolicies {
                            worker_material_assignment_allowed: current.runtime.worker_material_assignment_allowed,
                            queue: current.queue.discipline,
                            material: input.material,
                            tooling: input.tooling,
                        }),
                        ..CanonicalApparatusPatch::default()
                    },
                    canonical_command_metadata(&principal, &headers)?,
                )
                .await
                .map_err(canonical_apparatus_error)?;
            state.production_maps.notify_live();
            Ok(json_response(committed))
        }
        _ => Err(method_not_allowed()),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalMaterialPatchRequest {
    apparatus_id: ApparatusId,
    expected_revision: u64,
    material: MaterialExecutionPolicy,
    tooling: ToolingExecutionPolicy,
}

include!("raw_materials_parts/raw_material_assignments.rs");

include!("raw_materials_parts/raw_material_assignment_diagnostics.rs");

include!("raw_materials_parts/raw_material_intake_candidates.rs");

#[cfg(test)]
#[path = "raw_material_assignment_quantity_tests.rs"]
mod raw_material_assignment_quantity_tests;

include!("raw_materials_parts/raw_material_stock.rs");
