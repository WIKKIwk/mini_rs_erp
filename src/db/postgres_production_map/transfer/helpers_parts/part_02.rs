
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
        Some("paused") => {}
        Some("frozen") => return Err(ProductionMapError::OrderFrozen),
        Some("completed") => return Err(ProductionMapError::OrderAlreadyCompleted),
        _ => return Err(ProductionMapError::ApparatusTransferOrderNotPaused),
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

    let session = sqlx::query_as::<_, (String, String, String)>(
        "SELECT canonical_apparatus_id, order_id, status
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
        || session.2 != "paused"
    {
        return Err(ProductionMapError::ApparatusTransferSessionMismatch);
    }

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
    {
        return Err(ProductionMapError::ApparatusTransferProgressMismatch);
    }
    for update in &write.progress_batch_updates {
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
