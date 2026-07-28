use super::raw_material_details::{
    fill_raw_material_assignment_input, item_group_path, lookup_raw_material_detail,
};
use super::*;
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, RawMaterialEventQuery, RawMaterialEventScope,
};
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
}

pub async fn raw_material_start_requirements(
    State(state): State<AppState>,
    Query(query): Query<RawMaterialStartRequirementsQuery>,
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
    if query.order_id.trim().is_empty() || query.apparatus.trim().is_empty() {
        return Err(bad_request("apparatus and order_id are required"));
    }
    let staged_barcodes = raw_material_state_barcodes_for_order_apparatus(
        &state,
        &query.order_id,
        &query.apparatus,
    )
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
            queue_state::apparatus_titles_match(&assignment.apparatus, &query.apparatus)
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
                && queue_state::apparatus_titles_match(&assignment.apparatus, apparatus)
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
        .filter(|placement| {
            placement
                .apparatus
                .iter()
                .any(|candidate| queue_state::apparatus_titles_match(candidate, apparatus))
        })
        .map(|placement| placement.barcode)
        .collect())
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
                assignments.retain(|assignment| {
                    assignment.order_id.trim() == query.order_id.trim()
                });
            }
            if !query.apparatus.trim().is_empty() {
                assignments.retain(|assignment| {
                    queue_state::apparatus_titles_match(
                        &assignment.apparatus,
                        &query.apparatus,
                    )
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
    if principal.role == PrincipalRole::Aparatchi {
        let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
        if !queue_state::apparatus_matches_assigned(apparatus, &assigned_apparatus) {
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

    let staged_barcodes = raw_material_state_barcodes_for_order_apparatus(
        &state,
        order_id,
        apparatus,
    )
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
            && queue_state::apparatus_titles_match(&assignment.apparatus, apparatus)
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
            queue_state::apparatus_titles_match(&assignment.apparatus, &requested_apparatus)
        })
        .ok_or_else(|| {
            production_map_error(ProductionMapError::RawMaterialAssignmentNotFound)
        })?;
    let staged_barcodes = raw_material_state_barcodes_for_order_apparatus(
        &state,
        &assignment.order_id,
        &assignment.apparatus,
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
        apparatus: Some(assignment.apparatus.trim().to_string()),
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
            "apparatus": assignment.apparatus.trim(),
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
            "apparatus": assignment.apparatus,
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
        object.insert(
            "stock_qty".to_string(),
            serde_json::json!(stock_qty),
        );
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
            raw_material_assignment_quantities(
                &assignment,
                Some(&stock("available", ""))
            ),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(
            raw_material_assignment_quantities(
                &assignment,
                Some(&stock("in_use", "zakaz-1"))
            ),
            (1_000.0, 0.0, 1_000.0)
        );
        assert_eq!(
            raw_material_assignment_quantities(
                &assignment,
                Some(&stock("consumed", "zakaz-1"))
            ),
            (1_000.0, 1_000.0, 0.0)
        );
        assert_eq!(
            raw_material_assignment_quantities(
                &assignment,
                Some(&stock("in_use", "zakaz-2"))
            ),
            (0.0, 0.0, 0.0)
        );
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
