
/// Apparatus order sequences are stored server-side so every device (admin
/// and worker) sees the same queue order.
pub async fn production_map_sequence(
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
            Capability::ApparatusQueueRead,
            Capability::RawMaterialAssign,
            Capability::QolipManage,
        ],
    )
    .await?;
    match method {
        Method::GET => {
            let snapshot = state
                .production_maps
                .live_snapshot_shared()
                .await
                .map_err(production_map_error)?;
            let snapshot = super::training::merge_worker_training_snapshot_shared(
                &state, &principal, snapshot,
            )
            .await
            .map_err(super::training::training_workspace_error)?;
            let order_customers = production_map_order_customers(&state, &snapshot.maps).await;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "sequences": &snapshot.sequences,
                "visible_order_ids": &snapshot.visible_order_ids,
                "queue_states": &snapshot.queue_states,
                "stage_states": &snapshot.stage_states,
                "queue_policies": &snapshot.queue_policies,
                "queue_action_controls": &snapshot.queue_action_controls,
                "order_statuses": &snapshot.order_statuses,
                "order_controls": &snapshot.order_controls,
                "frozen_orders_by_apparatus": &snapshot.frozen_orders_by_apparatus,
                "order_customers": order_customers,
            })))
        }
        Method::PUT => {
            authorize_any_capability(
                &state,
                &headers,
                &[Capability::AdminAccess, Capability::ProductionMapManage],
            )
            .await?;
            let input: ApparatusSequencePutRequest = parse_json(&body)?;
            if input.apparatus.trim().is_empty() {
                return Err(bad_request("apparatus is required"));
            }
            state
                .production_maps
                .set_apparatus_sequence(&input.apparatus, input.order_ids)
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({"ok": true})))
        }
        _ => Err(method_not_allowed()),
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ApparatusQueuePolicyPutRequest {
    apparatus_id: ApparatusId,
    expected_revision: u64,
    discipline: QueueDiscipline,
}

/// Apparatus queue policy controls whether a worker must follow the saved
/// sequence or can pick any ready order. Pechat stays strict in the service.
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
        .raw_material_assignments()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment() -> RawMaterialAssignment {
        RawMaterialAssignment {
            order_id: "zakaz-001".to_string(),
            apparatus_id: ApparatusId::new("apparatus:catalog:lam-001".to_string())
                .expect("valid apparatus id"),
            apparatus: "Laminatsiya (historical title)".to_string(),
            barcode: "RM-001".to_string(),
            item_code: "FILM-001".to_string(),
            item_name: "Film".to_string(),
            item_group: "Rulon".to_string(),
            assigned_by_role: "material_taminotchi".to_string(),
            assigned_by_ref: "worker-001".to_string(),
            assigned_by_display_name: "Worker".to_string(),
            assigned_at: "2026-08-19T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn renamed_historical_display_name_still_matches_by_canonical_id() {
        assert!(raw_material_assignment_matches_order_apparatus(
            &assignment(),
            "zakaz-001",
            "apparatus:catalog:lam-001",
        ));
    }

    #[test]
    fn title_only_apparatus_mismatch_cannot_select_completion_material() {
        let assignment = assignment();

        assert!(!raw_material_assignment_matches_order_apparatus(
            &assignment,
            "zakaz-001",
            "Laminatsiya (historical title)",
        ));
        assert!(!raw_material_assignment_matches_order_apparatus(
            &assignment,
            "zakaz-001",
            "Laminatsiya",
        ));
    }
}
