
pub(super) async fn training_material_assignments_for_principal(
    state: &AppState,
    principal: &Principal,
    order_id: &str,
    apparatus: &str,
) -> Result<Option<Vec<serde_json::Value>>, TrainingWorkspaceError> {
    let Some(_) = training_map_for_principal(state, principal, order_id, apparatus).await? else {
        return Ok(None);
    };
    let store = state
        .training_workspace
        .as_ref()
        .ok_or(TrainingWorkspaceError::StoreFailed)?;
    let order_id = order_id.trim();
    let apparatus = canonical_training_apparatus(apparatus)?;
    Ok(Some(
        store.raw_material_assignments(order_id, &apparatus).await?,
    ))
}

pub(super) async fn training_raw_material_start_requirements(
    state: &AppState,
    principal: &Principal,
    order_id: &str,
    apparatus: &str,
    material_barcodes: &str,
) -> Result<Option<serde_json::Value>, TrainingWorkspaceError> {
    let Some(assignments) =
        training_material_assignments_for_principal(state, principal, order_id, apparatus).await?
    else {
        return Ok(None);
    };
    let assigned_barcodes = assignments
        .iter()
        .filter_map(|assignment| assignment.get("barcode"))
        .filter_map(serde_json::Value::as_str)
        .map(normalize_training_barcode)
        .filter(|barcode| !barcode.is_empty())
        .collect::<BTreeSet<_>>();
    let scanned_barcodes = material_barcodes
        .split(',')
        .map(normalize_training_barcode)
        .filter(|barcode| !barcode.is_empty())
        .collect::<BTreeSet<_>>();
    let matched_scan_count = scanned_barcodes.intersection(&assigned_barcodes).count();
    let scan_satisfied = assigned_barcodes.is_empty()
        || (!scanned_barcodes.is_empty()
            && scanned_barcodes.is_subset(&assigned_barcodes)
            && scanned_barcodes == assigned_barcodes);
    let assigned_barcodes = assigned_barcodes.into_iter().collect::<Vec<_>>();
    Ok(Some(serde_json::json!({
        "policy": "state_all",
        "requires_material": !assigned_barcodes.is_empty(),
        "requirement_groups": [],
        "assigned_barcodes": assigned_barcodes.clone(),
        "staged_barcodes": assigned_barcodes.clone(),
        "eligible_barcodes": assigned_barcodes.clone(),
        "required_scan_count": assigned_barcodes.len(),
        "matched_scan_count": matched_scan_count,
        "assignments_satisfied": true,
        "scan_satisfied": scan_satisfied,
        "assignments": assignments.clone(),
        "start_assignments": assignments,
    })))
}

fn normalize_training_barcode(barcode: &str) -> String {
    barcode.trim().to_ascii_uppercase()
}

pub(super) async fn merge_worker_training_maps(
    state: &AppState,
    principal: &Principal,
    maps: &mut Vec<ProductionMapSaved>,
) -> Result<(), TrainingWorkspaceError> {
    let overlay = worker_training_overlay(state, principal).await?;
    if overlay.active_apparatuses.is_empty() {
        return Ok(());
    }
    maps.retain(|saved| {
        !overlay
            .active_apparatuses
            .iter()
            .any(|apparatus| training_map_has_apparatus(saved, apparatus))
    });
    maps.extend(overlay.maps);
    Ok(())
}

pub(super) async fn merge_worker_training_snapshot(
    state: &AppState,
    principal: &Principal,
    snapshot: &mut ProductionMapLiveSnapshot,
) -> Result<(), TrainingWorkspaceError> {
    let overlay = worker_training_overlay(state, principal).await?;
    if overlay.active_apparatuses.is_empty() {
        return Ok(());
    }

    let hidden_order_ids = snapshot
        .maps
        .iter()
        .filter(|saved| {
            overlay
                .active_apparatuses
                .iter()
                .any(|apparatus| training_map_has_apparatus(saved, apparatus))
        })
        .map(|saved| saved.map.id.trim().to_string())
        .filter(|order_id| !order_id.is_empty())
        .collect::<BTreeSet<_>>();
    snapshot
        .maps
        .retain(|saved| !hidden_order_ids.contains(saved.map.id.trim()));
    snapshot.maps.extend(overlay.maps.clone());
    snapshot
        .sequences
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &overlay.active_apparatuses));
    snapshot
        .visible_order_ids
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &overlay.active_apparatuses));
    snapshot
        .queue_states
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &overlay.active_apparatuses));
    snapshot
        .queue_action_controls
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &overlay.active_apparatuses));
    snapshot
        .stage_states
        .retain(|order_id, _| !hidden_order_ids.contains(order_id));
    snapshot.queue_policies.retain(|policy| {
        !is_training_apparatus(policy.apparatus_id.as_str(), &overlay.active_apparatuses)
    });
    snapshot
        .order_statuses
        .retain(|order_id, _| !hidden_order_ids.contains(order_id));
    snapshot
        .order_controls
        .retain(|order_id, _| !hidden_order_ids.contains(order_id));
    snapshot.sequences.extend(overlay.sequences);
    snapshot.visible_order_ids.extend(overlay.visible_order_ids);
    snapshot.queue_states.extend(overlay.queue_states);
    snapshot
        .queue_action_controls
        .extend(overlay.queue_action_controls);
    snapshot.queue_policies.extend(overlay.queue_policies);
    snapshot.order_statuses.extend(overlay.order_statuses);
    Ok(())
}
