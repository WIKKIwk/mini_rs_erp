async fn validate_wip_route_claim_tx(
    tx: &mut Transaction<'_, Postgres>,
    write: &QueueActionProgressWrite,
    current: &OrderProgressBatch,
    proposed: &OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    let resumed_input = write.event.action == queue_state::ApparatusQueueAction::Resume
        && current.wip_status == OrderProgressBatchWipStatus::Waiting
        && (current.is_layer_remainder()
            || write.event.payload_json.get("wip_input_expected_batch").is_some());
    if resumed_input {
        let session = write.session.as_ref().ok_or(ProductionMapError::WipRouteChanged)?;
        let detached = sqlx::query_scalar::<_, bool>(
            "SELECT status IN ('paused', 'roll_detached')
                AND COALESCE(payload_json->>'input_progress_batch_id', '') = ''
                AND (COALESCE(payload_json->>'lamination_astatka_shift_closed', '') = 'true'
                  OR ($5 AND COALESCE(payload_json->>'layer_remainder_detached_input_batch_id', '') = $4))
             FROM mini_order_run_sessions
             WHERE session_id = $1 AND order_id = $2 AND canonical_apparatus_id = $3
               AND stage_node_id = $6
             FOR UPDATE",
        ).bind(&session.session_id).bind(&write.event.order_id).bind(&write.apparatus)
            .bind(&current.parent_batch_id).bind(current.is_layer_remainder())
            .bind(&session.stage_node_id).fetch_optional(&mut **tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if detached != Some(true) {
            return Err(ProductionMapError::ProgressBatchNotResumable);
        }
    }
    if current.wip_status == OrderProgressBatchWipStatus::InUse {
        // A resumed input belongs to its existing session and route. Do not
        // re-resolve it into a different operation after a graph edit.
        if let Some(binding) = current.payload_json.get("wip_route_binding") {
            if proposed.payload_json.get("wip_route_binding") != Some(binding) {
                return Err(ProductionMapError::WipRouteChanged);
            }
            let binding: WipInputRoute = serde_json::from_value(binding.clone())
                .map_err(|_| ProductionMapError::WipRouteChanged)?;
            validate_claim_session(write, proposed, &binding)?;
        }
        return Ok(());
    }
    // Producer resumes use the original producer session rather than the
    // next-stage route. Start/Merge may revisit the same physical apparatus
    // at a different graph occurrence, so canonical apparatus equality alone
    // must never bypass their map and source witness checks.
    if !resumed_input && current.apparatus == write.apparatus
        && !matches!(
            write.event.action,
            queue_state::ApparatusQueueAction::Start | queue_state::ApparatusQueueAction::Merge
        )
    {
        return Ok(());
    }
    if !resumed_input && !matches!(
        write.event.action,
        queue_state::ApparatusQueueAction::Start | queue_state::ApparatusQueueAction::Merge
    ) {
        // Established Resume/handoff/recovery paths keep their original pin.
        // They cannot introduce a new route or replace one on an already
        // mounted input. Legacy unpinned sessions retain the ownership gates.
        if proposed.payload_json.get("wip_route_binding")
            != current.payload_json.get("wip_route_binding")
        {
            return Err(ProductionMapError::WipRouteChanged);
        }
        if let Some(binding) = current.payload_json.get("wip_route_binding") {
            let binding: WipInputRoute = serde_json::from_value(binding.clone())
                .map_err(|_| ProductionMapError::WipRouteChanged)?;
            validate_claim_session(write, proposed, &binding)?;
            let session = write
                .session
                .as_ref()
                .ok_or(ProductionMapError::WipRouteChanged)?;
            let linked = sqlx::query_scalar::<_, bool>(
                "SELECT COALESCE(payload_json->>'input_progress_batch_id', '') = $4
                 FROM mini_order_run_sessions
                 WHERE session_id = $1 AND order_id = $2
                   AND canonical_apparatus_id = $3 AND stage_node_id = $5
                 FOR UPDATE",
            )
            .bind(&session.session_id)
            .bind(&write.event.order_id)
            .bind(&write.apparatus)
            .bind(&current.batch_id)
            .bind(&binding.stage_node_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
            if linked != Some(true) {
                return Err(ProductionMapError::WipRouteChanged);
            }
        }
        return Ok(());
    }
    // Every new downstream claim needs a preview witness, including a legacy
    // self-consumed output recovered in memory. Absence cannot bypass the
    // transaction's map and batch revalidation.
    let expected = write
        .event
        .payload_json
        .get("wip_input_expected_batch")
        .ok_or(ProductionMapError::WipRouteChanged)?;
    let expected: OrderProgressBatch = serde_json::from_value(expected.clone())
        .map_err(|_| ProductionMapError::WipRouteChanged)?;
    if current != &expected {
        return Err(ProductionMapError::WipRouteChanged);
    }
    let expected_fingerprint = write
        .event
        .payload_json
        .get("wip_input_map_fingerprint")
        .and_then(serde_json::Value::as_str)
        .ok_or(ProductionMapError::WipRouteChanged)?;
    let map = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT map_json FROM mini_production_maps WHERE id = $1 FOR UPDATE",
    )
    .bind(&write.event.order_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::WipRouteChanged)?;
    let map: ProductionMapDefinition =
        serde_json::from_value(map).map_err(|_| ProductionMapError::StoreFailed)?;
    if production_map_fingerprint(&map) != expected_fingerprint {
        return Err(ProductionMapError::WipRouteChanged);
    }
    let normalized = normalized_route_batch(current);
    if !actionable_wip(&normalized) {
        return Err(ProductionMapError::ProgressBatchNotAccepted);
    }
    let route = resolve_wip_input_route(&map, &normalized)?;
    let binding: WipInputRoute = serde_json::from_value(
        proposed
            .payload_json
            .get("wip_route_binding")
            .cloned()
            .ok_or(ProductionMapError::WipRouteChanged)?,
    )
    .map_err(|_| ProductionMapError::WipRouteChanged)?;
    if binding.source_stage_node_id != route.source_stage_node_id
        || binding.consumer_apparatus_ids != route.consumer_apparatus_ids
        || binding.map_fingerprint != route.map_fingerprint
        || binding.remapped != route.remapped
        || !stage_node_ids_match_for_map(&map, &binding.stage_node_id, &route.stage_node_id)
        || !claim_preserves_source(&normalized, proposed)
    {
        return Err(ProductionMapError::WipRouteChanged);
    }
    validate_claim_session(write, proposed, &binding)
}

fn claim_preserves_source(source: &OrderProgressBatch, proposed: &OrderProgressBatch) -> bool {
    let mut restored = proposed.clone();
    restored.wip_status = source.wip_status;
    restored.current_apparatus = source.current_apparatus.clone();
    restored.current_location = source.current_location.clone();
    restored.used_by_apparatus = source.used_by_apparatus.clone();
    restored.used_by_session_id = source.used_by_session_id.clone();
    restored.processed_by_apparatus = source.processed_by_apparatus.clone();
    restored.processed_by_session_id = source.processed_by_session_id.clone();
    for key in [
        "wip_route_binding",
        "wip_in_use_at_unix",
        "wip_processed_at_unix",
    ] {
        if let Some(value) = source.payload_json.get(key) {
            restored.payload_json[key] = value.clone();
        } else if let Some(payload) = restored.payload_json.as_object_mut() {
            payload.remove(key);
        }
    }
    restored.refresh_status_detail();
    restored == *source
}

fn validate_claim_session(
    write: &QueueActionProgressWrite,
    proposed: &OrderProgressBatch,
    binding: &WipInputRoute,
) -> Result<(), ProductionMapError> {
    let session = write
        .session
        .as_ref()
        .ok_or(ProductionMapError::WipRouteChanged)?;
    if !binding.consumer_apparatus_ids.contains(&write.apparatus)
        || session.apparatus != write.apparatus
        || session.order_id != write.event.order_id
        || session.stage_node_id != binding.stage_node_id
        || proposed.used_by_apparatus != write.apparatus
        || proposed.used_by_session_id != session.session_id
    {
        return Err(ProductionMapError::WipRouteChanged);
    }
    Ok(())
}
