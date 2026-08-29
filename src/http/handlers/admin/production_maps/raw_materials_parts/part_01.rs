
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
    fn from_stock(
        stock: &RawMaterialStockEntry,
        item: &SupplierItem,
    ) -> Self {
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
            let barcode = input.barcode.clone();
            let assigned = match state
                .production_maps
                .assign_raw_material_to_order(input, &queue_action_actor(&principal))
                .await
            {
                Ok(assigned) => assigned,
                Err(ProductionMapError::RawMaterialAlreadyAssigned) => {
                    return Err(raw_material_already_assigned_error(&state, &barcode).await);
                }
                Err(error) => return Err(production_map_error(error)),
            };
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
            let stock = raw_material_unlink_stock_guard(&state, &principal, &existing).await?;
            let removed = match state
                .production_maps
                .unlink_raw_material_assignment(input)
                .await
            {
                Ok(removed) => removed,
                Err(ProductionMapError::RawMaterialAssignmentLocked) => {
                    let status = stock
                        .as_ref()
                        .map(|entry| entry.status.as_str())
                        .unwrap_or("locked");
                    return Err(
                        raw_material_assignment_locked_error(&state, &existing, status).await,
                    );
                }
                Err(error) => return Err(production_map_error(error)),
            };
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

async fn raw_material_already_assigned_error(state: &AppState, barcode: &str) -> AdminError {
    let normalized_barcode = barcode.trim().to_ascii_uppercase();
    let assignment = state
        .production_maps
        .raw_material_assignments()
        .await
        .ok()
        .and_then(|assignments| {
            assignments.into_iter().find(|assignment| {
                assignment.barcode.trim().to_ascii_uppercase() == normalized_barcode
            })
        });
    let Some(assignment) = assignment else {
        return bad_request("raw_material_already_assigned");
    };

    let order_title = raw_material_order_title(state, &assignment.order_id).await;

    let mut response = AdminErrorResponse::new("raw_material_already_assigned");
    response.order_title = Some(order_title);
    (StatusCode::BAD_REQUEST, Json(response))
}

async fn raw_material_order_title(state: &AppState, order_id: &str) -> String {
    let order_id = order_id.trim().to_string();
    state
        .production_maps
        .raw_map(&order_id)
        .await
        .ok()
        .flatten()
        .map(|order| {
            let title = order.title.trim();
            if title.is_empty() {
                let code = order.code.trim();
                if code.is_empty() {
                    let number = order.order_number.trim();
                    if number.is_empty() {
                        order.id.trim()
                    } else {
                        number
                    }
                } else {
                    code
                }
            } else {
                title
            }
            .to_string()
        })
        .unwrap_or(order_id)
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
