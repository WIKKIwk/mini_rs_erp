
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
    Ok(Some(serde_json::json!({
        "policy": "state_all",
        "requires_material": !assigned_barcodes.is_empty(),
        "requirement_groups": [],
        "assigned_barcodes": &assigned_barcodes,
        "staged_barcodes": &assigned_barcodes,
        "eligible_barcodes": &assigned_barcodes,
        "required_scan_count": assigned_barcodes.len(),
        "matched_scan_count": matched_scan_count,
        "assignments_satisfied": true,
        "scan_satisfied": scan_satisfied,
        "assignments": &assignments,
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

pub(super) async fn merge_worker_training_snapshot_shared(
    state: &AppState,
    principal: &Principal,
    mut snapshot: std::sync::Arc<ProductionMapLiveSnapshot>,
) -> Result<std::sync::Arc<ProductionMapLiveSnapshot>, TrainingWorkspaceError> {
    let overlay = worker_training_overlay(state, principal).await?;
    if overlay.active_apparatuses.is_empty() {
        return Ok(snapshot);
    }
    merge_worker_training_overlay(std::sync::Arc::make_mut(&mut snapshot), overlay);
    Ok(snapshot)
}

fn merge_worker_training_overlay(
    snapshot: &mut ProductionMapLiveSnapshot,
    overlay: WorkerTrainingOverlay,
) {
    let WorkerTrainingOverlay {
        active_apparatuses,
        maps,
        sequences,
        visible_order_ids,
        queue_states,
        queue_policies,
        queue_action_controls,
        order_statuses,
        ..
    } = overlay;
    if active_apparatuses.is_empty() {
        return;
    }

    let hidden_order_ids = snapshot
        .maps
        .iter()
        .filter(|saved| {
            active_apparatuses
                .iter()
                .any(|apparatus| training_map_has_apparatus(saved, apparatus))
        })
        .map(|saved| saved.map.id.trim().to_string())
        .filter(|order_id| !order_id.is_empty())
        .collect::<BTreeSet<_>>();
    snapshot
        .maps
        .retain(|saved| !hidden_order_ids.contains(saved.map.id.trim()));
    snapshot
        .sequences
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &active_apparatuses));
    snapshot
        .visible_order_ids
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &active_apparatuses));
    snapshot
        .queue_states
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &active_apparatuses));
    snapshot
        .queue_action_controls
        .retain(|apparatus, _| !is_training_apparatus(apparatus, &active_apparatuses));
    snapshot
        .stage_states
        .retain(|order_id, _| !hidden_order_ids.contains(order_id));
    snapshot.queue_policies.retain(|policy| {
        !is_training_apparatus(policy.apparatus_id.as_str(), &active_apparatuses)
    });
    snapshot
        .order_statuses
        .retain(|order_id, _| !hidden_order_ids.contains(order_id));
    snapshot
        .order_controls
        .retain(|order_id, _| !hidden_order_ids.contains(order_id));
    snapshot.maps.extend(maps);
    snapshot.sequences.extend(sequences);
    snapshot.visible_order_ids.extend(visible_order_ids);
    snapshot.queue_states.extend(queue_states);
    snapshot.queue_action_controls.extend(queue_action_controls);
    snapshot.queue_policies.extend(queue_policies);
    snapshot.order_statuses.extend(order_statuses);
}
