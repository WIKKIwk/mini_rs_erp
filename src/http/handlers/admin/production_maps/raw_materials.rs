use super::raw_material_details::{
    apparatus_id_matches_text, apparatus_id_matches_text_value, assigned_apparatus_contains,
    fill_raw_material_assignment_input, item_group_path, lookup_raw_material_detail,
    raw_material_rulon_match_metrics, require_material_item_group_scope,
    require_material_warehouse_scope, resolve_raw_material_stock_item,
    validate_rulon_size_for_apparatus_map,
};
use super::*;
use crate::core::inventory_movements::RawMaterialStatePlacement;
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, RawMaterialEventQuery, RawMaterialEventScope,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, serde::Deserialize)]
pub struct RawMaterialStartRequirementsQuery {
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    material_barcodes: String,
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
    let requirements = state
        .production_maps
        .raw_material_start_requirements(
            &query.apparatus,
            &query.order_id,
            &staged_barcodes,
            &query.material_barcodes,
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
    let assignment_barcodes = state
        .production_maps
        .raw_material_assignments()
        .await
        .map_err(production_map_error)?
        .into_iter()
        .filter(|assignment| {
            assignment.order_id.trim() == order_id.trim()
                && apparatus_id_matches_text(&assignment.apparatus_id, apparatus)
        })
        .map(|assignment| assignment.barcode)
        .collect::<Vec<_>>();
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
                .set_apparatus_material_rule(input, &state.apparatus_groups)
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
    Query(query): Query<RawMaterialAssignmentsQuery>,
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
            if let Some(training_assignments) =
                super::super::training::training_material_assignments_for_principal(
                    &state,
                    &principal,
                    &query.order_id,
                    &query.apparatus,
                )
                .await
                .map_err(super::super::training::training_workspace_error)?
            {
                return Ok(json_response(training_assignments));
            }
            if principal.role == PrincipalRole::MaterialTaminotchi
                && !query.apparatus.trim().is_empty()
            {
                let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
                if !assigned_apparatus_contains(&query.apparatus, &assigned_apparatus) {
                    return Err(production_map_error(
                        ProductionMapError::ApparatusNotAssigned,
                    ));
                }
            }
            let mut assignments = state
                .production_maps
                .raw_material_assignments()
                .await
                .map_err(production_map_error)?;
            if principal.role == PrincipalRole::MaterialTaminotchi {
                assignments =
                    material_scoped_raw_material_assignments(&state, &principal, assignments)
                        .await?;
            }
            if !query.order_id.trim().is_empty() {
                assignments
                    .retain(|assignment| assignment.order_id.trim() == query.order_id.trim());
            }
            if !query.apparatus.trim().is_empty() {
                assignments.retain(|assignment| {
                    apparatus_id_matches_text(&assignment.apparatus_id, &query.apparatus)
                });
            }
            sort_raw_material_assignments(&mut assignments);
            Ok(json_response(
                raw_material_assignment_responses(&state, assignments).await,
            ))
        }
        Method::POST => {
            require_capability(&state, &principal, Capability::RawMaterialAssign).await?;
            let input: RawMaterialAssignmentInput = parse_json(&body)?;
            let (input, warehouse) =
                fill_raw_material_assignment_input(&state, &principal, input).await?;
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
        Method::DELETE => {
            require_capability(&state, &principal, Capability::RawMaterialAssign).await?;
            let input: RawMaterialAssignmentDeleteInput = parse_json(&body)?;
            let existing = find_raw_material_assignment(&state, &input.order_id, &input.barcode)
                .await?
                .ok_or_else(|| {
                    production_map_error(ProductionMapError::RawMaterialAssignmentNotFound)
                })?;
            let stock = raw_material_unlink_stock_guard(&state, &existing.barcode).await?;
            let removed = state
                .production_maps
                .unlink_raw_material_assignment(input)
                .await
                .map_err(production_map_error)?;
            record_raw_material_unlink_event(&state, &principal, &removed).await;
            record_raw_material_unassignment_event(&state, &principal, &removed, stock.as_ref())
                .await;
            if let Some(stock) = stock {
                state
                    .warehouse_events
                    .notify_updated(&stock.warehouse, "raw_material_assignment_unlink");
            }
            Ok(json_response(serde_json::json!({
                "ok": true,
                "assignment": raw_material_assignment_response(&state, removed).await,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub async fn raw_material_assignment_orders(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, AdminError> {
    authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::RawMaterialAssign,
        ],
    )
    .await?;
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    state
        .production_maps
        .raw_material_assignment_orders()
        .await
        .map(json_response)
        .map_err(production_map_error)
}

pub async fn raw_material_assignment_candidates(
    State(state): State<AppState>,
    Query(query): Query<RawMaterialAssignmentsQuery>,
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
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let order_id = query.order_id.trim();
    if order_id.is_empty() {
        return Err(bad_request("order_id is required"));
    }

    let order = state
        .production_maps
        .raw_material_assignment_orders()
        .await
        .map_err(production_map_error)?
        .into_iter()
        .find(|saved| saved.map.id.trim() == order_id)
        .ok_or_else(|| production_map_error(ProductionMapError::MapNotFound))?;
    let stock = if principal.role == PrincipalRole::MaterialTaminotchi {
        material_scoped_raw_material_stock(&state, &principal, "", 500).await?
    } else {
        state
            .gscale
            .raw_material_stock("", 500)
            .await
            .map_err(|_| server_error("raw material stock fetch failed"))?
    };
    let assigned_barcodes = state
        .production_maps
        .raw_material_assignments()
        .await
        .map_err(production_map_error)?
        .into_iter()
        .map(|assignment| assignment.barcode.trim().to_ascii_uppercase())
        .collect::<BTreeSet<_>>();
    let item_codes = stock
        .iter()
        .map(|entry| entry.item_code.trim().to_string())
        .filter(|code| !code.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let items = state
        .admin
        .items_by_codes(&item_codes)
        .await
        .map_err(|_| server_error("raw material items fetch failed"))?
        .into_iter()
        .map(|item| (item.code.trim().to_ascii_lowercase(), item))
        .collect::<BTreeMap<_, _>>();
    let groups = state
        .admin
        .item_group_tree()
        .await
        .map_err(|_| server_error("item group tree fetch failed"))?;
    let assigned_item_groups = if principal.role == PrincipalRole::MaterialTaminotchi {
        Some(
            state
                .admin
                .principal_assigned_item_group_scope(&principal)
                .await
                .map_err(|_| server_error("material item group scope fetch failed"))?,
        )
    } else {
        None
    };
    let assigned_apparatus = if principal.role == PrincipalRole::MaterialTaminotchi {
        Some(state.admin.principal_assigned_apparatus(&principal).await)
    } else {
        None
    };
    let requested_apparatus = query.apparatus.trim();
    if assigned_apparatus.as_ref().is_some_and(|assigned| {
        !requested_apparatus.is_empty()
            && !assigned_apparatus_contains(requested_apparatus, assigned)
    }) {
        return Err(production_map_error(
            ProductionMapError::ApparatusNotAssigned,
        ));
    }

    let mut apparatus_options_by_group = BTreeMap::<String, Vec<String>>::new();
    let mut candidates = Vec::<RawMaterialAssignmentCandidateResponse>::new();
    for entry in stock {
        let barcode = entry.barcode.trim();
        if barcode.is_empty()
            || !entry.status.trim().eq_ignore_ascii_case("available")
            || !entry.reserved_order_id.trim().is_empty()
            || assigned_barcodes.contains(&barcode.to_ascii_uppercase())
        {
            continue;
        }
        let Some(item) = items.get(&entry.item_code.trim().to_ascii_lowercase()) else {
            continue;
        };
        if assigned_item_groups.as_ref().is_some_and(|assigned| {
            !assigned
                .iter()
                .any(|group| group.trim().eq_ignore_ascii_case(item.item_group.trim()))
        }) {
            continue;
        }
        let group_path = item_group_path(&groups, &item.item_group);
        if group_path.is_empty() {
            continue;
        }
        let group_key = group_path
            .iter()
            .map(|group| group.trim().to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("\u{1f}");
        let apparatus_options = if let Some(options) = apparatus_options_by_group.get(&group_key) {
            options.clone()
        } else {
            let options = state
                .production_maps
                .raw_material_assignment_apparatus_options(order_id, &group_path)
                .await
                .map_err(production_map_error)?;
            let options = filter_raw_material_apparatus_options(
                options,
                requested_apparatus,
                assigned_apparatus.as_deref(),
            );
            apparatus_options_by_group.insert(group_key, options.clone());
            options
        };
        let apparatus_options = apparatus_options
            .into_iter()
            .filter(|apparatus| {
                validate_rulon_size_for_apparatus_map(
                    &order.map,
                    apparatus,
                    &entry,
                    item,
                    &group_path,
                )
                .is_ok()
            })
            .collect::<Vec<_>>();
        if apparatus_options.is_empty() {
            continue;
        }
        let (order_width_mm, roll_width_mm, leftover_width_mm, match_type) =
            match apparatus_options.iter().find_map(|apparatus| {
                raw_material_rulon_match_metrics(&order.map, apparatus, &entry, item, &group_path)
            }) {
                Some((order_width, roll_width, leftover_width)) => (
                    Some(order_width),
                    Some(roll_width),
                    Some(leftover_width),
                    if leftover_width <= 0.001 {
                        "exact_width".to_string()
                    } else {
                        "closest_width".to_string()
                    },
                ),
                None => (None, None, None, "compatible".to_string()),
            };
        candidates.push(RawMaterialAssignmentCandidateResponse {
            barcode: barcode.to_string(),
            warehouse: entry.warehouse.trim().to_string(),
            item_code: entry.item_code.trim().to_string(),
            item_name: item.name.trim().to_string(),
            item_group: item.item_group.trim().to_string(),
            qty: entry.qty,
            uom: entry.uom.trim().to_string(),
            apparatus_options,
            order_width_mm,
            roll_width_mm,
            leftover_width_mm,
            match_type,
        });
    }
    candidates.sort_by(|left, right| {
        raw_material_candidate_match_priority(&left.match_type)
            .cmp(&raw_material_candidate_match_priority(&right.match_type))
            .then_with(|| {
                left.leftover_width_mm
                    .unwrap_or(f64::INFINITY)
                    .partial_cmp(&right.leftover_width_mm.unwrap_or(f64::INFINITY))
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| {
                left.item_name
                    .to_ascii_lowercase()
                    .cmp(&right.item_name.to_ascii_lowercase())
            })
            .then_with(|| left.barcode.cmp(&right.barcode))
    });
    Ok(json_response(candidates))
}

fn raw_material_candidate_match_priority(match_type: &str) -> u8 {
    match match_type.trim() {
        "exact_width" => 0,
        "closest_width" => 1,
        _ => 2,
    }
}

fn filter_raw_material_apparatus_options(
    options: Vec<String>,
    requested_apparatus: &str,
    assigned_apparatus: Option<&[String]>,
) -> Vec<String> {
    options
        .into_iter()
        .filter(|apparatus| {
            assigned_apparatus
                .is_none_or(|assigned| assigned_apparatus_contains(apparatus, assigned))
        })
        .filter(|apparatus| {
            requested_apparatus.is_empty()
                || apparatus_id_matches_text_value(apparatus, requested_apparatus)
        })
        .collect()
}

pub async fn raw_material_assignment_candidate_orders(
    State(state): State<AppState>,
    Query(query): Query<RawMaterialAssignmentsQuery>,
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
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let barcode = query.barcode.trim();
    if barcode.is_empty() {
        return Err(bad_request("barcode is required"));
    }

    let (stock, item) = resolve_raw_material_stock_item(&state, barcode).await?;
    require_material_item_group_scope(&state, &principal, &item.item_group).await?;
    require_material_warehouse_scope(&state, &principal, &stock.warehouse).await?;
    if !stock.status.trim().eq_ignore_ascii_case("available")
        || !stock.reserved_order_id.trim().is_empty()
    {
        return Ok(json_response(Vec::<
            RawMaterialAssignmentOrderCandidateResponse,
        >::new()));
    }
    let normalized_barcode = barcode.to_ascii_uppercase();
    if state
        .production_maps
        .raw_material_assignments()
        .await
        .map_err(production_map_error)?
        .iter()
        .any(|assignment| assignment.barcode.trim().to_ascii_uppercase() == normalized_barcode)
    {
        return Ok(json_response(Vec::<
            RawMaterialAssignmentOrderCandidateResponse,
        >::new()));
    }

    let groups = state
        .admin
        .item_group_tree()
        .await
        .map_err(|_| server_error("item group tree fetch failed"))?;
    let group_path = item_group_path(&groups, &item.item_group);
    if group_path.is_empty() {
        return Ok(json_response(Vec::<
            RawMaterialAssignmentOrderCandidateResponse,
        >::new()));
    }
    let active_orders = state
        .production_maps
        .raw_material_assignment_orders()
        .await
        .map_err(production_map_error)?;
    let assigned_apparatus = if principal.role == PrincipalRole::MaterialTaminotchi {
        Some(state.admin.principal_assigned_apparatus(&principal).await)
    } else {
        None
    };
    let requested_apparatus = query.apparatus.trim();
    if assigned_apparatus.as_ref().is_some_and(|assigned| {
        !requested_apparatus.is_empty()
            && !assigned_apparatus_contains(requested_apparatus, assigned)
    }) {
        return Err(production_map_error(
            ProductionMapError::ApparatusNotAssigned,
        ));
    }
    let mut candidates = Vec::<RawMaterialAssignmentOrderCandidateResponse>::new();
    for order in active_orders {
        let apparatus_options = state
            .production_maps
            .raw_material_assignment_apparatus_options(&order.map.id, &group_path)
            .await
            .map_err(production_map_error)?;
        let apparatus_options = filter_raw_material_apparatus_options(
            apparatus_options,
            requested_apparatus,
            assigned_apparatus.as_deref(),
        )
        .into_iter()
        .filter(|apparatus| {
            validate_rulon_size_for_apparatus_map(&order.map, apparatus, &stock, &item, &group_path)
                .is_ok()
        })
        .collect::<Vec<_>>();
        if apparatus_options.is_empty() {
            continue;
        }
        candidates.push(RawMaterialAssignmentOrderCandidateResponse {
            order,
            apparatus_options,
        });
    }
    candidates.sort_by(|left, right| {
        left.order
            .map
            .code
            .to_ascii_lowercase()
            .cmp(&right.order.map.code.to_ascii_lowercase())
            .then_with(|| left.order.map.id.cmp(&right.order.map.id))
    });
    Ok(json_response(candidates))
}

fn sort_raw_material_assignments(assignments: &mut [RawMaterialAssignment]) {
    assignments.sort_by(|left, right| {
        let left_title = if left.item_name.trim().is_empty() {
            left.item_code.trim()
        } else {
            left.item_name.trim()
        };
        let right_title = if right.item_name.trim().is_empty() {
            right.item_code.trim()
        } else {
            right.item_name.trim()
        };
        left_title
            .to_ascii_lowercase()
            .cmp(&right_title.to_ascii_lowercase())
            .then_with(|| left.barcode.cmp(&right.barcode))
    });
}

pub async fn raw_material_intake_candidates(
    State(state): State<AppState>,
    Query(query): Query<RawMaterialAssignmentsQuery>,
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
    let order_id = query.order_id.trim();
    let apparatus = query.apparatus.trim();
    if order_id.is_empty() || apparatus.is_empty() {
        return Err(bad_request("apparatus and order_id are required"));
    }
    if order_id.starts_with("training-") {
        super::super::training::training_material_assignments_for_principal(
            &state, &principal, order_id, apparatus,
        )
        .await
        .map_err(super::super::training::training_workspace_error)?
        .ok_or_else(|| not_found("training_order_not_found"))?;
        return Ok(json_response(Vec::<serde_json::Value>::new()));
    }
    if principal.role == PrincipalRole::Aparatchi {
        let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
        if !assigned_apparatus_contains(apparatus, &assigned_apparatus) {
            return Err(production_map_error(
                ProductionMapError::ApparatusNotAssigned,
            ));
        }
    }
    if !state
        .production_maps
        .raw_material_intake_is_available(order_id, apparatus)
        .await
        .map_err(production_map_error)?
    {
        return Ok(json_response(Vec::<serde_json::Value>::new()));
    }

    let staged_barcodes =
        raw_material_state_barcodes_for_order_apparatus(&state, order_id, apparatus)
            .await?
            .into_iter()
            .map(|barcode| barcode.trim().to_ascii_uppercase())
            .collect::<BTreeSet<_>>();
    let groups = state
        .admin
        .item_group_tree()
        .await
        .map_err(|_| server_error("item group tree fetch failed"))?;
    let mut assignments = state
        .production_maps
        .raw_material_assignments()
        .await
        .map_err(production_map_error)?;
    assignments.retain(|assignment| {
        assignment.order_id.trim() == order_id
            && apparatus_id_matches_text(&assignment.apparatus_id, apparatus)
            && staged_barcodes.contains(&assignment.barcode.trim().to_ascii_uppercase())
    });

    let mut candidates = Vec::new();
    for assignment in assignments {
        let stock = state
            .gscale
            .raw_material_stock_by_barcode(&assignment.barcode)
            .await
            .map_err(|_| server_error("raw material stock fetch failed"))?;
        let Some(stock) = stock else {
            continue;
        };
        if !stock.status.trim().eq_ignore_ascii_case("available")
            || !stock.reserved_order_id.trim().is_empty()
        {
            continue;
        }
        if !state
            .production_maps
            .raw_material_matches_apparatus_rule(
                apparatus,
                &assignment.item_group,
                item_group_path(&groups, &assignment.item_group),
            )
            .await
            .map_err(production_map_error)?
        {
            continue;
        }
        candidates.push(assignment);
    }
    sort_raw_material_assignments(&mut candidates);
    Ok(json_response(
        raw_material_assignment_responses(&state, candidates).await,
    ))
}

/// Receives one additional physical raw-material roll while the order is running.
pub async fn raw_material_intake(
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
    require_capability(&state, &principal, Capability::ApparatusQueueManage).await?;
    let input: RawMaterialAssignmentInput = parse_json(&body)?;
    let requested_apparatus = input.apparatus.trim().to_string();
    if requested_apparatus.is_empty() {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    }
    let assignment = find_raw_material_assignment(&state, &input.order_id, &input.barcode)
        .await?
        .filter(|assignment| {
            apparatus_id_matches_text(&assignment.apparatus_id, &requested_apparatus)
        })
        .ok_or_else(|| production_map_error(ProductionMapError::RawMaterialAssignmentNotFound))?;
    let staged_barcodes = raw_material_state_barcodes_for_order_apparatus(
        &state,
        &assignment.order_id,
        assignment.apparatus_id.as_str(),
    )
    .await?;
    if !staged_barcodes
        .iter()
        .any(|barcode| barcode.trim().eq_ignore_ascii_case(&assignment.barcode))
    {
        return Err(production_map_error(
            ProductionMapError::RawMaterialStateNotReady,
        ));
    }
    let stock = state
        .gscale
        .raw_material_stock_by_barcode(&assignment.barcode)
        .await
        .map_err(|_| server_error("raw material stock fetch failed"))?
        .ok_or_else(|| production_map_error(ProductionMapError::RawMaterialStockUnavailable))?;
    if !stock.status.trim().eq_ignore_ascii_case("available")
        || !stock.reserved_order_id.trim().is_empty()
    {
        return Err(production_map_error(
            ProductionMapError::RawMaterialStockUnavailable,
        ));
    }
    let warehouse = stock.warehouse.trim().to_string();
    let (input, _) = fill_raw_material_assignment_input(&state, &principal, input).await?;
    let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
    let actor = queue_action_actor(&principal);
    let (assignment, mut warehouses) = state
        .production_maps
        .receive_raw_material_for_active_order(input, &assigned_apparatus, &actor)
        .await
        .map_err(production_map_error)?;

    // Non-Postgres stores do not own the stock ledger transaction. Production's
    // Postgres store returns the touched warehouse from the atomic transaction.
    if warehouses.is_empty() {
        warehouses = state
            .gscale
            .mark_raw_material_stock_in_use(
                std::slice::from_ref(&assignment.barcode),
                &assignment.order_id,
            )
            .await
            .map_err(raw_material_stock_status_error)?
            .into_iter()
            .map(|stock| stock.warehouse)
            .filter(|warehouse| !warehouse.trim().is_empty())
            .collect();
    }
    if warehouses.is_empty() && !warehouse.trim().is_empty() {
        warehouses.push(warehouse);
    }
    warehouses.sort();
    warehouses.dedup();
    for warehouse in warehouses {
        state
            .warehouse_events
            .notify_updated(&warehouse, "raw_material_intake");
    }
    Ok(json_response(
        raw_material_assignment_response(&state, assignment).await,
    ))
}

async fn record_raw_material_unassignment_event(
    state: &AppState,
    principal: &Principal,
    assignment: &RawMaterialAssignment,
    stock: Option<&RawMaterialStockEntry>,
) {
    let Some(store) = state.raw_material_events.as_ref() else {
        return;
    };
    let Some(stock) = stock else {
        return;
    };
    let actor = queue_action_actor(principal);
    let draft = RawMaterialEventDraft {
        idempotency_key: format!(
            "order_unreserved:{}:{}:{}",
            assignment.barcode.trim().to_ascii_uppercase(),
            assignment.order_id.trim(),
            actor.ref_.trim()
        ),
        event_type: "order_unreserved".to_string(),
        warehouse: stock.warehouse.trim().to_string(),
        barcode: assignment.barcode.trim().to_string(),
        item_code: assignment.item_code.trim().to_string(),
        item_name: assignment.item_name.trim().to_string(),
        qty_delta: 0.0,
        uom: stock.uom.trim().to_string(),
        stock_status_before: Some(stock.status.trim().to_string()),
        stock_status_after: Some(stock.status.trim().to_string()),
        order_id: Some(assignment.order_id.trim().to_string()),
        apparatus: Some(assignment.apparatus_id.to_string()),
        actor_role: actor.role.trim().to_string(),
        actor_ref: actor.ref_.trim().to_string(),
        actor_display_name: actor.display_name.trim().to_string(),
        owner_role: if assignment.assigned_by_role.trim() == "material_taminotchi" {
            "material_taminotchi".to_string()
        } else {
            String::new()
        },
        owner_ref: if assignment.assigned_by_role.trim() == "material_taminotchi" {
            assignment.assigned_by_ref.trim().to_string()
        } else {
            String::new()
        },
        owner_display_name: if assignment.assigned_by_role.trim() == "material_taminotchi" {
            assignment.assigned_by_display_name.trim().to_string()
        } else {
            String::new()
        },
        source_type: "order_assignment".to_string(),
        source_id: assignment.order_id.trim().to_string(),
        source_line_ref: Some(assignment.barcode.trim().to_string()),
        correlation_id: None,
        payload_json: serde_json::json!({
            "order_id": assignment.order_id.trim(),
            "apparatus_id": assignment.apparatus_id.as_str(),
            "barcode": assignment.barcode.trim(),
            "item_group": assignment.item_group.trim(),
            "source_receipt_id": stock.source_receipt_id.trim(),
        }),
    };
    if let Err(error) = store.record_event(draft).await {
        tracing::warn!(%error, "raw material unassignment event record failed");
    }
}

async fn find_raw_material_assignment(
    state: &AppState,
    order_id: &str,
    barcode: &str,
) -> Result<Option<RawMaterialAssignment>, AdminError> {
    let order_id = order_id.trim();
    let barcode = barcode.trim();
    if order_id.is_empty() || barcode.is_empty() {
        return Err(production_map_error(
            ProductionMapError::RawMaterialInvalidInput,
        ));
    }
    let normalized = barcode.to_ascii_uppercase();
    Ok(state
        .production_maps
        .raw_material_assignments()
        .await
        .map_err(production_map_error)?
        .into_iter()
        .find(|assignment| {
            assignment.order_id.trim() == order_id
                && assignment.barcode.trim().to_ascii_uppercase() == normalized
        }))
}

async fn raw_material_unlink_stock_guard(
    state: &AppState,
    barcode: &str,
) -> Result<Option<RawMaterialStockEntry>, AdminError> {
    let stock = state
        .gscale
        .raw_material_stock_by_barcode(barcode)
        .await
        .map_err(|_| server_error("raw material stock fetch failed"))?;
    if let Some(stock) = stock.as_ref() {
        let status = stock.status.trim();
        if !status.is_empty() && !status.eq_ignore_ascii_case("available") {
            return Err(production_map_error(
                ProductionMapError::RawMaterialAssignmentLocked,
            ));
        }
    }
    Ok(stock)
}

async fn record_raw_material_unlink_event(
    state: &AppState,
    principal: &Principal,
    assignment: &RawMaterialAssignment,
) {
    let Some(engine) = state.mini_engine.as_ref() else {
        return;
    };
    let actor = queue_action_actor(principal);
    let actor_key = format!("{}:{}", actor.role.trim(), actor.ref_.trim());
    let event = crate::engine::EngineEventDraft {
        domain: "raw_material_assignment".to_string(),
        action: "unlinked".to_string(),
        entity_id: assignment.order_id.trim().to_string(),
        actor_key,
        idempotency_key: String::new(),
        payload_json: serde_json::json!({
            "order_id": assignment.order_id,
            "apparatus_id": assignment.apparatus_id,
            "barcode": assignment.barcode,
            "item_code": assignment.item_code,
            "item_name": assignment.item_name,
            "item_group": assignment.item_group,
            "assigned_by_role": assignment.assigned_by_role,
            "assigned_by_ref": assignment.assigned_by_ref,
            "unlinked_by_role": actor.role,
            "unlinked_by_ref": actor.ref_,
            "unlinked_by_display_name": actor.display_name,
        }),
    };
    let _ = engine.record_event(&event).await;
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
        let stock_status = stock
            .as_ref()
            .map(|entry| entry.status.trim())
            .unwrap_or_default();
        let stock_qty = stock
            .as_ref()
            .map(|entry| entry.qty)
            .filter(|qty| qty.is_finite() && *qty > 0.0)
            .unwrap_or(0.0);
        let (received_qty, consumed_qty, remaining_qty) =
            raw_material_assignment_quantities(&assignment, stock.as_ref());
        object.insert(
            "stock_status".to_string(),
            serde_json::Value::String(stock_status.to_string()),
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
            serde_json::Value::String(
                stock
                    .as_ref()
                    .map(|entry| entry.warehouse.clone())
                    .unwrap_or_default(),
            ),
        );
        object.insert("stock_qty".to_string(), serde_json::json!(stock_qty));
        object.insert(
            "stock_uom".to_string(),
            serde_json::Value::String(
                stock
                    .as_ref()
                    .map(|entry| entry.uom.clone())
                    .unwrap_or_default(),
            ),
        );
        object.insert("received_qty".to_string(), serde_json::json!(received_qty));
        object.insert("consumed_qty".to_string(), serde_json::json!(consumed_qty));
        object.insert(
            "remaining_qty".to_string(),
            serde_json::json!(remaining_qty),
        );
    }
    value
}

fn raw_material_assignment_quantities(
    assignment: &RawMaterialAssignment,
    stock: Option<&RawMaterialStockEntry>,
) -> (f64, f64, f64) {
    let Some(stock) = stock else {
        return (0.0, 0.0, 0.0);
    };
    let qty = if stock.qty.is_finite() && stock.qty > 0.0 {
        stock.qty
    } else {
        0.0
    };
    let belongs_to_order = stock
        .reserved_order_id
        .trim()
        .eq_ignore_ascii_case(assignment.order_id.trim());
    let received = belongs_to_order
        && matches!(
            stock.status.trim().to_ascii_lowercase().as_str(),
            "in_use" | "consumed"
        );
    let consumed = belongs_to_order && stock.status.trim().eq_ignore_ascii_case("consumed");
    let received_qty = if received { qty } else { 0.0 };
    let consumed_qty = if consumed { qty } else { 0.0 };
    (
        received_qty,
        consumed_qty,
        (received_qty - consumed_qty).max(0.0),
    )
}

#[cfg(test)]
mod raw_material_assignment_quantity_tests {
    use super::*;

    fn assignment() -> RawMaterialAssignment {
        RawMaterialAssignment {
            order_id: "zakaz-1".to_string(),
            apparatus_id: crate::core::apparatus_standard::ApparatusId::new(
                "apparatus:test:pechat-1",
            )
            .unwrap(),
            apparatus: "Pechat".to_string(),
            barcode: "ROLL-1000".to_string(),
            item_code: "RULON".to_string(),
            item_name: "Rulon".to_string(),
            item_group: "Rulon".to_string(),
            assigned_by_role: "aparatchi".to_string(),
            assigned_by_ref: "worker-1".to_string(),
            assigned_by_display_name: "Worker".to_string(),
            assigned_at: String::new(),
        }
    }

    fn stock(status: &str, order_id: &str) -> RawMaterialStockEntry {
        RawMaterialStockEntry {
            qty: 1_000.0,
            status: status.to_string(),
            reserved_order_id: order_id.to_string(),
            ..RawMaterialStockEntry::default()
        }
    }

    #[test]
    fn quantity_ledger_preserves_received_consumed_remaining_invariant() {
        let assignment = assignment();
        assert_eq!(
            raw_material_assignment_quantities(&assignment, Some(&stock("available", ""))),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(
            raw_material_assignment_quantities(&assignment, Some(&stock("in_use", "zakaz-1"))),
            (1_000.0, 0.0, 1_000.0)
        );
        assert_eq!(
            raw_material_assignment_quantities(&assignment, Some(&stock("consumed", "zakaz-1"))),
            (1_000.0, 1_000.0, 0.0)
        );
        assert_eq!(
            raw_material_assignment_quantities(&assignment, Some(&stock("in_use", "zakaz-2"))),
            (0.0, 0.0, 0.0)
        );
    }

    #[test]
    fn staged_placement_matches_canonical_id_not_display_name() {
        let placement = RawMaterialStatePlacement {
            barcode: "ROLL-1000".to_string(),
            location_id: "location:state:pechat".to_string(),
            location_name: "Pechat oldi".to_string(),
            apparatus_ids: vec!["apparatus:catalog:pechat-001".to_string()],
            apparatus: vec!["7 ta rangli pechat - A".to_string()],
        };

        assert!(state_placement_matches_apparatus(
            &placement,
            "apparatus:catalog:pechat-001"
        ));
        assert!(!state_placement_matches_apparatus(
            &placement,
            "7 ta rangli pechat - A"
        ));
    }
}

pub async fn raw_material_stock(
    State(state): State<AppState>,
    Query(query): Query<ItemQuery>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
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
    match method {
        Method::GET => {
            let limit = optional_search_limit(query.limit.as_deref(), 200, 500);
            let warehouse = query.warehouse.as_deref().unwrap_or("");
            if principal.role == PrincipalRole::MaterialTaminotchi {
                return material_scoped_raw_material_stock(&state, &principal, warehouse, limit)
                    .await
                    .map(json_response);
            }
            state
                .gscale
                .raw_material_stock(warehouse, limit)
                .await
                .map(json_response)
                .map_err(|_| server_error("raw material stock fetch failed"))
        }
        Method::PUT => update_material_scoped_raw_material_stock(&state, &principal, &body).await,
        _ => Err(method_not_allowed()),
    }
}

#[derive(Debug, serde::Deserialize)]
struct RawMaterialStockUpdateRequest {
    #[serde(default)]
    barcode: String,
    #[serde(default)]
    item_code: String,
    #[serde(default)]
    qty: f64,
}

async fn update_material_scoped_raw_material_stock(
    state: &AppState,
    principal: &Principal,
    body: &[u8],
) -> Result<Response, AdminError> {
    if principal.role != PrincipalRole::MaterialTaminotchi {
        return Err(forbidden());
    }
    require_capability(state, principal, Capability::RawMaterialAssign).await?;
    let request: RawMaterialStockUpdateRequest = parse_json(body)?;
    let barcode = request.barcode.trim();
    let item_code = request.item_code.trim();
    if barcode.is_empty() || item_code.is_empty() {
        return Err(bad_request("raw_material_stock_update_invalid"));
    }
    let current = state
        .gscale
        .raw_material_stock_by_barcode(barcode)
        .await
        .map_err(|_| server_error("raw material stock fetch failed"))?
        .ok_or_else(|| not_found("raw_material_stock_not_found"))?;
    let warehouses = material_warehouse_scope(state, principal).await?;
    if !warehouse_in_scope(&warehouses, &current.warehouse) {
        return Err(forbidden());
    }
    let has_assignment = state
        .production_maps
        .raw_material_assignments()
        .await
        .map_err(production_map_error)?
        .iter()
        .any(|assignment| assignment.barcode.trim().eq_ignore_ascii_case(barcode));
    if has_assignment
        || !current.status.trim().eq_ignore_ascii_case("available")
        || !current.reserved_order_id.trim().is_empty()
    {
        return Err(raw_material_stock_locked_error());
    }

    let items = state
        .admin
        .items_by_codes(&[item_code.to_string()])
        .await
        .map_err(|_| server_error("raw material item fetch failed"))?;
    let selected_item = items
        .into_iter()
        .find(|item| item.code.trim().eq_ignore_ascii_case(item_code))
        .ok_or_else(|| bad_request("raw_material_item_not_found"))?;
    if selected_item.uom.trim().is_empty()
        || !selected_item
            .uom
            .trim()
            .eq_ignore_ascii_case(current.uom.trim())
    {
        return Err(bad_request("raw_material_uom_mismatch"));
    }
    let assigned_groups = state
        .admin
        .principal_assigned_item_group_scope(principal)
        .await
        .map_err(|_| server_error("item group scope fetch failed"))?;
    if selected_item.item_group.trim().is_empty()
        || !assigned_groups.iter().any(|group| {
            group
                .trim()
                .eq_ignore_ascii_case(selected_item.item_group.trim())
        })
    {
        return Err(bad_request(
            "item group is not assigned to material taminotchi",
        ));
    }
    let actor = queue_action_actor(principal);
    let item_name = selected_item.name.trim();
    let updated = state
        .gscale
        .update_raw_material_stock(RawMaterialStockUpdateInput {
            barcode: barcode.to_string(),
            item_code: selected_item.code.trim().to_string(),
            item_name: if item_name.is_empty() {
                item_code.to_string()
            } else {
                item_name.to_string()
            },
            qty: request.qty,
            actor_role: actor.role,
            actor_ref: actor.ref_,
            actor_display_name: actor.display_name,
        })
        .await
        .map_err(raw_material_stock_update_error)?;
    state
        .warehouse_events
        .notify_updated(&updated.warehouse, "raw_material_stock_corrected");
    Ok(json_response(updated))
}

fn raw_material_stock_update_error(error: crate::core::gscale::GscaleServiceError) -> AdminError {
    match error {
        crate::core::gscale::GscaleServiceError::InvalidInput(detail)
            if detail == "raw_material_stock_not_found" =>
        {
            not_found(detail)
        }
        crate::core::gscale::GscaleServiceError::InvalidInput(detail)
            if detail == "raw_material_stock_locked" =>
        {
            raw_material_stock_locked_error()
        }
        crate::core::gscale::GscaleServiceError::InvalidInput(detail) => bad_request(detail),
        _ => server_error("raw material stock update failed"),
    }
}

pub(super) fn raw_material_stock_locked_error() -> AdminError {
    (
        StatusCode::CONFLICT,
        Json(AdminErrorResponse::new("raw_material_stock_locked")),
    )
}

include!("raw_materials_history.rs");
