/// Explains why a scanned roll is absent from the assignable-material list.
///
/// Candidate endpoints intentionally return only assignable stock. This
/// read-only endpoint keeps the filtering fail-closed while exposing the
/// exact rejection (including the two widths involved in a roll mismatch).
pub async fn raw_material_assignment_diagnostics(
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
    let mut diagnostic = RawMaterialAssignmentDiagnosticResponse::from_stock(&stock, &item);
    let normalized_barcode = barcode.to_ascii_uppercase();
    let requested_order_id = query.order_id.trim();

    let assignments = state
        .production_maps
        .raw_material_assignments()
        .await
        .map_err(production_map_error)?;
    if let Some(assignment) = assignments
        .into_iter()
        .find(|assignment| assignment.barcode.trim().to_ascii_uppercase() == normalized_barcode)
    {
        diagnostic.reason =
            if assignment.order_id.trim() == requested_order_id && !requested_order_id.is_empty() {
                "raw_material_already_assigned_to_order".to_string()
            } else {
                "raw_material_already_assigned".to_string()
            };
        diagnostic.order_id = Some(assignment.order_id.trim().to_string());
        diagnostic.order_title = Some(raw_material_order_title(&state, &assignment.order_id).await);
        diagnostic.apparatus = Some(assignment.apparatus_id.to_string());
        return Ok(json_response(diagnostic));
    }

    if !stock.status.trim().eq_ignore_ascii_case("available")
        || !stock.reserved_order_id.trim().is_empty()
    {
        diagnostic.reason = "raw_material_stock_unavailable".to_string();
        return Ok(json_response(diagnostic));
    }

    let groups = state
        .admin
        .item_group_tree()
        .await
        .map_err(|_| server_error("item group tree fetch failed"))?;
    let group_path = item_group_path(&groups, &item.item_group);
    if group_path.is_empty() {
        diagnostic.reason = "raw_material_group_not_allowed".to_string();
        return Ok(json_response(diagnostic));
    }

    let active_orders = state
        .production_maps
        .raw_material_assignment_orders()
        .await
        .map_err(production_map_error)?;
    let has_active_orders = !active_orders.is_empty();
    let selected_orders = if requested_order_id.is_empty() {
        active_orders
    } else {
        let Some(order) = active_orders
            .into_iter()
            .find(|saved| saved.map.id.trim() == requested_order_id)
        else {
            diagnostic.reason = "raw_material_order_not_active".to_string();
            diagnostic.order_id = Some(requested_order_id.to_string());
            return Ok(json_response(diagnostic));
        };
        vec![order]
    };

    let assigned_apparatus = if principal.role == PrincipalRole::MaterialTaminotchi {
        Some(state.principal_assigned_apparatus(&principal).await)
    } else {
        None
    };
    let requested_apparatus = query.apparatus.trim();
    if assigned_apparatus.as_ref().is_some_and(|assigned| {
        !requested_apparatus.is_empty()
            && !assigned_apparatus_contains(requested_apparatus, assigned)
    }) {
        diagnostic.reason = "apparatus_not_assigned".to_string();
        diagnostic.apparatus = Some(requested_apparatus.to_string());
        return Ok(json_response(diagnostic));
    }

    let mut best_failure: Option<RawMaterialAssignmentDiagnosticResponse> = None;
    for order in selected_orders {
        let all_apparatus_options = state
            .production_maps
            .raw_material_assignment_apparatus_options(&order.map.id, &group_path)
            .await
            .map_err(production_map_error)?;
        let apparatus_options = filter_raw_material_apparatus_options(
            all_apparatus_options.clone(),
            requested_apparatus,
            assigned_apparatus.as_deref(),
        );
        if apparatus_options.is_empty() {
            if !requested_order_id.is_empty() {
                diagnostic.reason = "raw_material_group_not_allowed".to_string();
                diagnostic.order_id = Some(order.map.id.trim().to_string());
                diagnostic.order_title = Some(order.map.title.trim().to_string());
                diagnostic.apparatus_options = all_apparatus_options;
                diagnostic.apparatus =
                    (!requested_apparatus.is_empty()).then(|| requested_apparatus.to_string());
                return Ok(json_response(diagnostic));
            }
            continue;
        }

        for apparatus in &apparatus_options {
            match validate_rulon_size_for_apparatus_map(
                &state,
                &order.map,
                apparatus,
                &stock,
                &item,
                &group_path,
            )
            .await
            {
                Ok(()) => {
                    diagnostic.compatible = true;
                    diagnostic.reason = "compatible".to_string();
                    diagnostic.order_id = Some(order.map.id.trim().to_string());
                    diagnostic.order_title = Some(order.map.title.trim().to_string());
                    diagnostic.apparatus = Some(apparatus.clone());
                    diagnostic.apparatus_options = apparatus_options.clone();
                    if let Some((order_width, roll_width, _)) = raw_material_rulon_match_metrics(
                        &order.map,
                        apparatus,
                        &stock,
                        &item,
                        &group_path,
                    ) {
                        diagnostic.order_width_mm = Some(order_width);
                        diagnostic.roll_width_mm = Some(roll_width);
                        let bounds = roll_width_allowance_mm(&state, apparatus)
                            .await?
                            .map(|allowance| rulon_width_bounds(order_width, allowance));
                        diagnostic.minimum_width_mm =
                            Some(bounds.map_or(order_width, |(minimum, _)| minimum));
                        diagnostic.maximum_width_mm = bounds.map(|(_, maximum)| maximum);
                    }
                    return Ok(json_response(diagnostic));
                }
                Err((_, Json(error))) => {
                    let mut failure = diagnostic.clone();
                    failure.order_id = Some(order.map.id.trim().to_string());
                    failure.order_title = Some(order.map.title.trim().to_string());
                    failure.apparatus = Some(apparatus.clone());
                    failure.apparatus_options = apparatus_options.clone();
                    failure.order_width_mm = error.order_width_mm;
                    failure.roll_width_mm = error.roll_width_mm;
                    failure.minimum_width_mm = error.minimum_width_mm;
                    failure.maximum_width_mm = error.maximum_width_mm;
                    failure.reason = error.error;
                    let should_replace = best_failure.as_ref().is_none_or(|current| {
                        raw_material_diagnostic_reason_priority(&failure.reason)
                            < raw_material_diagnostic_reason_priority(&current.reason)
                    });
                    if should_replace {
                        best_failure = Some(failure);
                    }
                }
            }
        }
    }

    if let Some(failure) = best_failure {
        return Ok(json_response(failure));
    }
    diagnostic.reason = if requested_order_id.is_empty() {
        if has_active_orders {
            "no_compatible_active_order".to_string()
        } else {
            "raw_material_order_not_active".to_string()
        }
    } else {
        "raw_material_group_not_allowed".to_string()
    };
    diagnostic.order_id = (!requested_order_id.is_empty()).then(|| requested_order_id.to_string());
    Ok(json_response(diagnostic))
}

fn raw_material_diagnostic_reason_priority(reason: &str) -> u8 {
    match reason.trim() {
        "raw_material_roll_size_mismatch" => 0,
        "raw_material_roll_size_missing" => 1,
        "raw_material_group_not_allowed" => 2,
        "apparatus_not_assigned" => 3,
        _ => 4,
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
    if state
        .production_maps
        .raw_material_assignment_by_barcode(barcode)
        .await
        .map_err(production_map_error)?
        .is_some()
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
        Some(state.principal_assigned_apparatus(&principal).await)
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
        if principal.role == PrincipalRole::TayyorlovMasteri {
            let Some(preparation) = state.preparation.as_ref() else {
                return Ok(json_response(Vec::<
                    RawMaterialAssignmentOrderCandidateResponse,
                >::new()));
            };
            if !preparation
                .order_in_scope(&principal.ref_, &order.map.id)
                .await
                .map_err(|_| forbidden())?
            {
                continue;
            }
        }
        let apparatus_options = state
            .production_maps
            .raw_material_assignment_apparatus_options_for_map(&order.map, &group_path)
            .await
            .map_err(production_map_error)?;
        let filtered_apparatus = filter_raw_material_apparatus_options(
            apparatus_options,
            requested_apparatus,
            assigned_apparatus.as_deref(),
        );
        let mut apparatus_options = Vec::new();
        for apparatus in filtered_apparatus {
            if !rulon_width_restriction_applies(principal.role)
                || validate_rulon_size_for_apparatus_map(
                    &state,
                    &order.map,
                    &apparatus,
                    &stock,
                    &item,
                    &group_path,
                )
                .await
                .is_ok()
            {
                apparatus_options.push(apparatus);
            }
        }
        if apparatus_options.is_empty() {
            continue;
        }
        candidates.push(RawMaterialAssignmentOrderCandidateResponse {
            order,
            apparatus_options,
        });
    }
    candidates.sort_by(|left, right| {
        cmp_ascii_case_insensitive(&left.order.map.code, &right.order.map.code)
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
        cmp_ascii_case_insensitive(left_title, right_title)
            .then_with(|| left.barcode.cmp(&right.barcode))
    });
}
