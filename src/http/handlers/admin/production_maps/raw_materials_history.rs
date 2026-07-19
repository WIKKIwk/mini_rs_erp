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

pub(super) async fn material_warehouse_scope(
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

pub(super) fn warehouse_in_scope(assigned: &[String], warehouse: &str) -> bool {
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
