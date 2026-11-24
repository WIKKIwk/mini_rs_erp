/// Apparatus queue policy controls whether a worker must follow the saved
/// sequence or can pick any ready order, including configured printing equipment.
pub async fn production_map_queue_policies(
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
            Capability::ProductionMapRead,
            Capability::ApparatusQueueRead,
        ],
    )
    .await?;
    match method {
        Method::GET => {
            let policies = state
                .apparatus
                .list_runtime_configurations()
                .await
                .map_err(canonical_apparatus_error)?
                .into_iter()
                .map(|configuration| configuration.queue)
                .collect::<Vec<_>>();
            Ok(json_response(serde_json::json!({
                "ok": true,
                "policies": policies,
            })))
        }
        Method::PUT => {
            authorize_any_capability(
                &state,
                &headers,
                &[Capability::AdminAccess, Capability::ProductionMapManage],
            )
            .await?;
            let input: ApparatusQueuePolicyPutRequest = parse_json(&body)?;
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
                            worker_material_assignment_allowed: current
                                .runtime
                                .worker_material_assignment_allowed,
                            queue: input.discipline,
                            material: current.material.policy.clone(),
                            tooling: current.material.tooling.clone(),
                        }),
                        ..CanonicalApparatusPatch::default()
                    },
                    canonical_command_metadata(&principal, &headers)?,
                )
                .await
                .map_err(canonical_apparatus_error)?;
            state.production_maps.notify_live();
            Ok(json_response(serde_json::json!({
                "ok": true,
                "revision": committed,
            })))
        }
        _ => Err(method_not_allowed()),
    }
}

pub(super) async fn raw_material_barcodes_for_order_apparatus(
    state: &AppState,
    order_id: &str,
    apparatus_id: &str,
) -> Result<Vec<String>, AdminError> {
    let assignments = state
        .production_maps
        .raw_material_assignments_for_order(order_id)
        .await
        .map_err(production_map_error)?;
    Ok(assignments
        .into_iter()
        .filter(|assignment| {
            raw_material_assignment_matches_order_apparatus(assignment, order_id, apparatus_id)
        })
        .map(|assignment| assignment.barcode.trim().to_string())
        .filter(|barcode| !barcode.is_empty())
        .collect())
}

pub(super) async fn settle_completion_raw_materials_fallback(
    state: &AppState,
    order_id: &str,
    barcodes: &[String],
) -> Result<Vec<String>, AdminError> {
    let order_id = order_id.trim();
    let mut consumed_barcodes = Vec::new();
    let mut unused = Vec::new();
    let mut warehouses = std::collections::BTreeSet::new();
    for barcode in barcodes {
        let stock = state
            .gscale
            .raw_material_stock_by_barcode(barcode)
            .await
            .map_err(|_| server_error("raw material stock fetch failed"))?
            .ok_or_else(|| bad_request("raw_material_stock_unavailable"))?;
        let status = stock.status.trim().to_ascii_lowercase();
        let reservation = stock.reserved_order_id.trim();
        if matches!(status.as_str(), "in_use" | "consumed") && reservation == order_id {
            consumed_barcodes.push(stock.barcode);
        } else if status == "available" && (reservation.is_empty() || reservation == order_id) {
            warehouses.insert(stock.warehouse.trim().to_string());
            unused.push(stock.barcode);
        } else {
            return Err(bad_request("raw_material_stock_unavailable"));
        }
    }
    if !consumed_barcodes.is_empty() {
        for stock in state
            .gscale
            .mark_raw_material_stock_consumed(&consumed_barcodes, order_id)
            .await
            .map_err(raw_material_stock_status_error)?
        {
            if !stock.warehouse.trim().is_empty() {
                warehouses.insert(stock.warehouse.trim().to_string());
            }
        }
    }
    for barcode in unused {
        state
            .production_maps
            .unlink_raw_material_assignment_under_queue_guard(RawMaterialAssignmentDeleteInput {
                order_id: order_id.to_string(),
                barcode,
            })
            .await
            .map_err(production_map_error)?;
    }
    Ok(warehouses
        .into_iter()
        .filter(|warehouse| !warehouse.is_empty())
        .collect())
}

fn raw_material_assignment_matches_order_apparatus(
    assignment: &RawMaterialAssignment,
    order_id: &str,
    apparatus_id: &str,
) -> bool {
    // The assignment title is a historical/display snapshot; live completion
    // matching must use the canonical storage identity.
    assignment.order_id.trim() == order_id.trim()
        && queue_state::apparatus_ids_match(assignment.apparatus_id.as_str(), apparatus_id)
}
