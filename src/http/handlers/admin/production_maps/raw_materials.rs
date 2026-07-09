use super::raw_material_details::{fill_raw_material_assignment_input, lookup_raw_material_detail};
use super::*;
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, RawMaterialEventQuery, RawMaterialEventScope,
};
use std::collections::BTreeMap;

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
    if method != Method::GET {
        return Err(method_not_allowed());
    }
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

pub async fn raw_material_history(
    State(state): State<AppState>,
    Query(query): Query<RawMaterialHistoryQuery>,
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
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let Some(store) = state.raw_material_events.as_ref() else {
        return Ok(json_response(Vec::<serde_json::Value>::new()));
    };
    let limit = optional_search_limit(query.limit.as_deref(), 50, 200);
    let scope = if principal.role == PrincipalRole::MaterialTaminotchi {
        RawMaterialEventScope {
            enabled: true,
            warehouses: material_warehouse_scope(&state, &principal).await?,
        }
    } else {
        RawMaterialEventScope::default()
    };
    let owner_filtered = principal.role == PrincipalRole::MaterialTaminotchi;
    let events = store
        .events(
            scope,
            RawMaterialEventQuery {
                warehouse: query.warehouse.unwrap_or_default(),
                event_type: query.event_type.unwrap_or_default(),
                actor_role: String::new(),
                actor_ref: String::new(),
                owner_role: if owner_filtered {
                    "material_taminotchi".to_string()
                } else {
                    String::new()
                },
                owner_ref: if owner_filtered {
                    principal.ref_.trim().to_string()
                } else {
                    String::new()
                },
                limit,
            },
        )
        .await
        .map_err(|_| server_error("raw material history fetch failed"))?;
    Ok(json_response(events))
}

#[derive(Debug, serde::Deserialize)]
pub struct RawMaterialHistoryQuery {
    pub warehouse: Option<String>,
    pub event_type: Option<String>,
    pub limit: Option<String>,
}

async fn material_scoped_raw_material_stock(
    state: &AppState,
    principal: &Principal,
    warehouse: &str,
    limit: usize,
) -> Result<Vec<RawMaterialStockEntry>, AdminError> {
    let assigned = material_warehouse_scope(state, principal).await?;
    if assigned.is_empty() {
        return Ok(Vec::new());
    }
    let requested = warehouse.trim();
    if !requested.is_empty() {
        if !warehouse_in_scope(&assigned, requested) {
            return Ok(Vec::new());
        }
        return state
            .gscale
            .raw_material_stock(requested, limit)
            .await
            .map_err(|_| server_error("raw material stock fetch failed"));
    }
    let mut out = Vec::new();
    for warehouse in assigned {
        let remaining = limit.saturating_sub(out.len());
        if remaining == 0 {
            break;
        }
        let mut stock = state
            .gscale
            .raw_material_stock(warehouse.trim(), remaining)
            .await
            .map_err(|_| server_error("raw material stock fetch failed"))?;
        out.append(&mut stock);
    }
    Ok(out)
}

async fn material_scoped_raw_material_assignments(
    state: &AppState,
    principal: &Principal,
    assignments: Vec<RawMaterialAssignment>,
) -> Result<Vec<RawMaterialAssignment>, AdminError> {
    let assigned = material_warehouse_scope(state, principal).await?;
    if assigned.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for assignment in assignments {
        let stock = state
            .gscale
            .raw_material_stock_by_barcode(&assignment.barcode)
            .await
            .map_err(|_| server_error("raw material stock fetch failed"))?;
        if stock
            .as_ref()
            .map(|entry| warehouse_in_scope(&assigned, &entry.warehouse))
            .unwrap_or(false)
        {
            out.push(assignment);
        }
    }
    Ok(out)
}

async fn material_warehouse_scope(
    state: &AppState,
    principal: &Principal,
) -> Result<Vec<String>, AdminError> {
    Ok(state
        .warehouses
        .assigned_warehouse_names(principal)
        .await
        .map_err(warehouse_error)?
        .into_iter()
        .map(|warehouse| warehouse.trim().to_string())
        .filter(|warehouse| !warehouse.trim().is_empty())
        .collect())
}

fn warehouse_in_scope(assigned: &[String], warehouse: &str) -> bool {
    assigned
        .iter()
        .any(|assigned| assigned.trim().eq_ignore_ascii_case(warehouse.trim()))
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
    if method != Method::GET {
        return Err(method_not_allowed());
    }
    let detail = lookup_raw_material_detail(&state, &principal, &query.barcode).await?;
    let mut value = serde_json::to_value(detail).unwrap_or_else(|_| serde_json::json!({}));
    let normalized = query.barcode.trim().to_ascii_uppercase();
    let assignment = state
        .production_maps
        .raw_material_assignments()
        .await
        .map_err(production_map_error)?
        .into_iter()
        .find(|assignment| assignment.barcode.trim().to_ascii_uppercase() == normalized);
    if let Some(object) = value.as_object_mut() {
        if let Some(assignment) = assignment {
            let order_id = assignment.order_id.trim().to_string();
            object.insert(
                "assignment".to_string(),
                raw_material_assignment_response(&state, assignment.clone()).await,
            );
            if let Some(order) = state
                .production_maps
                .raw_map(&order_id)
                .await
                .map_err(production_map_error)?
            {
                object.insert("order".to_string(), serde_json::json!(order));
            }
            let queue_states = state
                .production_maps
                .apparatus_queue_states()
                .await
                .map_err(production_map_error)?;
            object.insert(
                "queue_states".to_string(),
                serde_json::json!(queue_states_for_order(queue_states, &order_id)),
            );
            let logs_by_order = state
                .production_maps
                .queue_action_logs_for_order(&order_id)
                .await
                .map_err(production_map_error)?;
            object.insert("logs".to_string(), serde_json::json!(logs_by_order));
        } else {
            object.insert("assignment".to_string(), serde_json::Value::Null);
            object.insert("order".to_string(), serde_json::Value::Null);
            object.insert("queue_states".to_string(), serde_json::json!({}));
            object.insert("logs".to_string(), serde_json::json!([]));
        }
    }
    Ok(json_response(value))
}

#[derive(Default, serde::Deserialize)]
pub struct RawMaterialAssignmentLookupQuery {
    #[serde(default)]
    barcode: String,
}

fn queue_states_for_order(
    queue_states: BTreeMap<String, BTreeMap<String, String>>,
    order_id: &str,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let order_id = order_id.trim();
    queue_states
        .into_iter()
        .filter_map(|(apparatus, states)| {
            states.get(order_id).map(|state| {
                (
                    apparatus,
                    BTreeMap::from([(order_id.to_string(), state.clone())]),
                )
            })
        })
        .collect()
}
