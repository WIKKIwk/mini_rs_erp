use super::raw_material_details::{fill_raw_material_assignment_input, lookup_raw_material_detail};
use super::*;

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
