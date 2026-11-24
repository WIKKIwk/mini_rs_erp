async fn verify_transfer_preconditions(
    tx: &mut Transaction<'_, Postgres>,
    write: &ProductionMapApparatusTransferWrite,
) -> Result<(), ProductionMapError> {
    let source_state = sqlx::query_scalar::<_, String>(
        "SELECT state
         FROM mini_queue_states
         WHERE canonical_apparatus_id = $1 AND order_id = $2
         FOR UPDATE",
    )
    .bind(write.record.from_apparatus.trim())
    .bind(write.record.order_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    match source_state.as_deref() {
        Some("paused" | "pending") => {}
        Some("frozen") => return Err(ProductionMapError::OrderFrozen),
        Some("completed") => return Err(ProductionMapError::OrderAlreadyCompleted),
        _ => return Err(ProductionMapError::ApparatusTransferOrderNotPaused),
    }
    if let Some(expected) = &write.expected_map {
        let current = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT map_json FROM mini_production_maps WHERE id = $1 FOR UPDATE",
        ).bind(write.record.order_id.trim()).fetch_one(&mut **tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let current: crate::core::production_map::ProductionMapDefinition =
            serde_json::from_value(current).map_err(|_| ProductionMapError::StoreFailed)?;
        if &current != expected { return Err(ProductionMapError::WipRouteChanged); }
    }

    let control_state = sqlx::query_scalar::<_, String>(
        "SELECT state
         FROM mini_order_control_states
         WHERE order_id = $1
         FOR UPDATE",
    )
    .bind(write.record.order_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    match control_state.as_deref() {
        Some("freeze_requested") => return Err(ProductionMapError::OrderFreezeRequested),
        Some("frozen") => return Err(ProductionMapError::OrderFrozen),
        _ => {}
    }

    let target_has_order = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM mini_queue_states
             WHERE canonical_apparatus_id = $1 AND order_id = $2
         )",
    )
    .bind(write.record.to_apparatus.trim())
    .bind(write.record.order_id.trim())
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if target_has_order {
        return Err(ProductionMapError::ApparatusTransferTargetConflict);
    }
    let target_session = sqlx::query_scalar::<_, String>(
        "SELECT session_id
         FROM mini_order_run_sessions
         WHERE canonical_apparatus_id = $1
           AND order_id = $2
           AND status IN ('active', 'paused', 'frozen', 'roll_detached')
         FOR UPDATE",
    )
    .bind(write.record.to_apparatus.trim())
    .bind(write.record.order_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if target_session.is_some() {
        return Err(ProductionMapError::ApparatusTransferTargetConflict);
    }

    let session = sqlx::query_as::<_, (String, String, String, serde_json::Value)>(
        "SELECT canonical_apparatus_id, order_id, status, payload_json
         FROM mini_order_run_sessions
         WHERE session_id = $1
         FOR UPDATE",
    )
    .bind(write.record.session_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::ApparatusTransferSessionNotFound)?;
    if session.1.trim() != write.record.order_id.trim()
        || session.0.trim() != write.record.from_apparatus.trim()
        || !(session.2 == "paused" || (write.record.preserves_output_history && session.2 == "roll_detached"))
    {
        return Err(ProductionMapError::ApparatusTransferSessionMismatch);
    }
    if let Some(expected) = &write.expected_session {
        let current = super::order_query_helpers::load_order_run_session(&mut **tx, &write.record.session_id)
            .await?.ok_or(ProductionMapError::ApparatusTransferSessionNotFound)?;
        if &current != expected { return Err(ProductionMapError::ApparatusTransferSessionMismatch); }
        let controls = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT record_json FROM mini_stage_order_controls WHERE order_id = $1 FOR UPDATE",
        ).bind(write.record.order_id.trim()).fetch_all(&mut **tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        for control in controls {
            let control: crate::core::production_map::OrderControlRecord =
                serde_json::from_value(control).map_err(|_| ProductionMapError::StoreFailed)?;
            if control.freeze_request.as_ref().is_some_and(|request| request.target_apparatus == write.record.from_apparatus) {
                match control.state {
                    crate::core::production_map::OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
                    crate::core::production_map::OrderControlState::FreezeRequested => return Err(ProductionMapError::OrderFreezeRequested),
                    _ => {}
                }
            }
        }
    }
    let requeued = session.3.get("requeued_at_tail")
        .and_then(serde_json::Value::as_bool) == Some(true);
    if (source_state.as_deref() == Some("pending") && !requeued)
        || source_state.as_deref() != Some(write.record.queue_state.as_str())
    {
        return Err(ProductionMapError::ApparatusTransferOrderNotPaused);
    }

    if write.record.preserves_output_history {
        if !write.session.is_print_transfer_continuation() || write.progress_batch.is_some()
            || !write.record.progress_batch_id.is_empty() || write.expected_map.is_none() || write.expected_session.is_none()
        {
            return Err(ProductionMapError::ApparatusTransferProgressMismatch);
        }
        let current = sqlx::query_scalar::<_, String>(
            "SELECT a.barcode FROM mini_raw_material_assignments a
             WHERE a.order_id=$1 AND a.canonical_apparatus_id=$2
               AND NOT EXISTS(SELECT 1 FROM mini_raw_material_stock s
                   WHERE lower(s.barcode)=lower(a.barcode) AND s.status='consumed')
             ORDER BY a.barcode FOR UPDATE OF a",
        ).bind(&write.record.order_id).bind(&write.record.from_apparatus)
            .fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        let normalize = |barcodes: Vec<String>| barcodes.into_iter()
            .map(|barcode| barcode.trim().to_ascii_uppercase()).collect::<std::collections::BTreeSet<_>>();
        if normalize(current) != normalize(write.raw_material_assignments.iter().map(|a| a.barcode.clone()).collect()) {
            return Err(ProductionMapError::RawMaterialAssignmentLocked);
        }
    } else if let Some(expected_batch) = &write.progress_batch {
        let batch = sqlx::query_as::<_, (String, String, String, String, String)>(
            "SELECT canonical_apparatus_id, order_id, session_id, action, status
             FROM mini_progress_batches
             WHERE batch_id = $1
             FOR UPDATE",
        )
        .bind(write.record.progress_batch_id.trim())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::ApparatusTransferProgressNotFound)?;
        if batch.1.trim() != write.record.order_id.trim()
            || batch.2.trim() != write.record.session_id.trim()
            || batch.0.trim() != write.record.from_apparatus.trim()
            || batch.3 != "pause"
            || batch.4 != "paused"
            || expected_batch.batch_id.trim() != write.record.progress_batch_id.trim()
        {
            return Err(ProductionMapError::ApparatusTransferProgressMismatch);
        }
    } else {
        let has_paused_batch = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM mini_progress_batches
             WHERE session_id = $1 AND canonical_apparatus_id = $2
               AND action = 'pause' AND status = 'paused')",
        )
        .bind(write.record.session_id.trim())
        .bind(write.record.from_apparatus.trim())
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if source_state.as_deref() != Some("pending")
            || !requeued
            || !write.record.progress_batch_id.is_empty()
            || has_paused_batch
        {
            return Err(ProductionMapError::ApparatusTransferProgressMismatch);
        }
    }
    for update in &write.progress_batch_updates {
        if write.record.preserves_output_history {
            let current = super::order_query_helpers::load_progress_batch(&mut **tx, &update.batch_id)
                .await?.ok_or(ProductionMapError::WipRouteChanged)?;
            if !write.expected_input_batches.contains(&current)
                || current.wip_status != crate::core::production_map::OrderProgressBatchWipStatus::InUse
                || current.used_by_session_id != write.session.session_id
                || current.used_by_apparatus != write.record.from_apparatus
            { return Err(ProductionMapError::WipRouteChanged); }
        }
        let update_order_id = sqlx::query_scalar::<_, String>(
            "SELECT order_id
             FROM mini_progress_batches
             WHERE batch_id = $1
             FOR UPDATE",
        )
        .bind(update.batch_id.trim())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::ApparatusTransferProgressMismatch)?;
        if update_order_id.trim() != write.record.order_id.trim() {
            return Err(ProductionMapError::ApparatusTransferProgressMismatch);
        }
    }
    Ok(())
}
