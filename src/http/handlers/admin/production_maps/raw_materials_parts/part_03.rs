
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
