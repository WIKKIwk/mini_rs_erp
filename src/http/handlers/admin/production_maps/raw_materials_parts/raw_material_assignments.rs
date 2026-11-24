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
            if principal.role == PrincipalRole::RezkaMaster {
                super::super::rezka_master::require_order_scope(&state, &principal, &query.order_id).await?;
                if !query.apparatus.is_empty()
                    && !state.principal_assigned_apparatus(&principal).await.contains(&query.apparatus)
                {
                    return Err(forbidden());
                }
            }
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
                let assigned_apparatus = state.principal_assigned_apparatus(&principal).await;
                if !assigned_apparatus_contains(&query.apparatus, &assigned_apparatus) {
                    return Err(production_map_error(
                        ProductionMapError::ApparatusNotAssigned,
                    ));
                }
            }
            let mut assignments = if query.order_id.trim().is_empty() {
                state.production_maps.raw_material_assignments().await
            } else {
                state.production_maps.raw_material_assignments_for_order(&query.order_id).await
            }.map_err(production_map_error)?;
            if principal.role == PrincipalRole::MaterialTaminotchi
                || principal.role == PrincipalRole::TayyorlovMasteri
            {
                assignments =
                    material_scoped_raw_material_assignments(&state, &principal, assignments,
                        principal.role == PrincipalRole::MaterialTaminotchi
                            && !query.order_id.trim().is_empty())
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
            require_tayyorlov_order_scope(&state, &principal, &input.order_id).await?;
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
            require_tayyorlov_order_scope(&state, &principal, &input.order_id).await?;
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

/// Tayyorlov masteri faqat o'ziga biriktirilgan homashyosi bor
/// orderlarga ulay oladi/yecha oladi. Boshqa rollarga tegmaydi.
async fn require_tayyorlov_order_scope(
    state: &AppState,
    principal: &Principal,
    order_id: &str,
) -> Result<(), AdminError> {
    if principal.role != PrincipalRole::TayyorlovMasteri {
        return Ok(());
    }
    let Some(preparation) = state.preparation.as_ref() else {
        return Err(forbidden());
    };
    let in_scope = preparation
        .order_in_scope(&principal.ref_, order_id)
        .await
        .map_err(|_| forbidden())?;
    if in_scope { Ok(()) } else { Err(forbidden()) }
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
    let orders = state
        .production_maps
        .raw_material_assignment_orders()
        .await
        .map_err(production_map_error)?;
    if principal.role != PrincipalRole::TayyorlovMasteri {
        return Ok(json_response(orders));
    }
    let Some(preparation) = state.preparation.as_ref() else {
        return Ok(json_response(Vec::<serde_json::Value>::new()));
    };
    let mut scoped = Vec::new();
    for order in orders {
        let in_scope = preparation
            .order_in_scope(&principal.ref_, &order.map.id)
            .await
            .unwrap_or(false);
        if in_scope {
            scoped.push(order);
        }
    }
    Ok(json_response(scoped))
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
    require_tayyorlov_order_scope(&state, &principal, &order.map.id).await?;
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
        if !apparatus_options_by_group.contains_key(&group_key) {
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
            apparatus_options_by_group.insert(group_key.clone(), options);
        }
        let apparatus_options = apparatus_options_by_group
            .get(&group_key)
            .expect("group options inserted above");
        let mut compatible_apparatus = Vec::with_capacity(apparatus_options.len());
        for apparatus in apparatus_options {
            if validate_rulon_size_for_apparatus_map(
                &state,
                &order.map,
                apparatus,
                &entry,
                item,
                &group_path,
            )
            .await
            .is_ok()
            {
                compatible_apparatus.push(apparatus.clone());
            }
        }
        let apparatus_options = compatible_apparatus;
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
                    if leftover_width.abs() <= 0.001 {
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
            .then_with(|| cmp_ascii_case_insensitive(&left.item_name, &right.item_name))
            .then_with(|| left.barcode.cmp(&right.barcode))
    });
    Ok(json_response(candidates))
}

fn cmp_ascii_case_insensitive(left: &str, right: &str) -> Ordering {
    left.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(right.bytes().map(|byte| byte.to_ascii_lowercase()))
}

fn raw_material_candidate_match_priority(match_type: &str) -> u8 {
    match match_type.trim() {
        "exact_width" => 0,
        "closest_width" => 1,
        _ => 2,
    }
}
