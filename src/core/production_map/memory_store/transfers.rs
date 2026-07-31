use super::*;

use super::super::queue_state;
use super::runs::session_qolip_codes;

pub(super) async fn apparatus_transfer_by_idempotency_key(
    store: &MemoryProductionMapStore,
    idempotency_key: &str,
) -> Result<Option<ProductionMapApparatusTransferRecord>, ProductionMapError> {
    Ok(store
        .apparatus_transfers
        .read()
        .await
        .get(idempotency_key.trim())
        .cloned())
}

pub(super) async fn commit_apparatus_transfer(
    store: &MemoryProductionMapStore,
    write: ProductionMapApparatusTransferWrite,
) -> Result<ProductionMapApparatusTransferRecord, ProductionMapError> {
    if let Some(existing) = store
        .apparatus_transfers
        .read()
        .await
        .get(write.record.idempotency_key.trim())
        .cloned()
    {
        return Ok(existing);
    }

    let order_id = write.record.order_id.trim();
    let from = write.record.from_apparatus.trim();
    let to = write.record.to_apparatus.trim();
    let source_state = store
        .queue_states
        .read()
        .await
        .get(from)
        .and_then(|states| states.get(order_id))
        .map(|state| state.trim().to_string());
    if source_state.as_deref() != Some("paused") {
        return Err(ProductionMapError::ApparatusTransferOrderNotPaused);
    }
    if store
        .queue_states
        .read()
        .await
        .get(to)
        .is_some_and(|states| states.contains_key(order_id))
    {
        return Err(ProductionMapError::ApparatusTransferTargetConflict);
    }

    let current_session = store
        .order_run_sessions
        .read()
        .await
        .get(write.record.session_id.trim())
        .cloned()
        .ok_or(ProductionMapError::ApparatusTransferSessionNotFound)?;
    if current_session.order_id.trim() != order_id
        || !queue_state::apparatus_titles_match(&current_session.apparatus, from)
        || current_session.status != OrderRunStatus::Paused
    {
        return Err(ProductionMapError::ApparatusTransferSessionMismatch);
    }

    let current_batch = store
        .order_progress_batches
        .read()
        .await
        .get(write.record.progress_batch_id.trim())
        .cloned()
        .ok_or(ProductionMapError::ApparatusTransferProgressNotFound)?;
    if current_batch.order_id.trim() != order_id
        || current_batch.session_id.trim() != write.record.session_id.trim()
        || !queue_state::apparatus_titles_match(&current_batch.apparatus, from)
        || current_batch.action != queue_state::ApparatusQueueAction::Pause
        || current_batch.status != OrderProgressBatchStatus::Paused
    {
        return Err(ProductionMapError::ApparatusTransferProgressMismatch);
    }
    for update in &write.progress_batch_updates {
        let current = store
            .order_progress_batches
            .read()
            .await
            .get(update.batch_id.trim())
            .cloned()
            .ok_or(ProductionMapError::ApparatusTransferProgressMismatch)?;
        if current.order_id.trim() != order_id {
            return Err(ProductionMapError::ApparatusTransferProgressMismatch);
        }
    }

    {
        let mut reservations = store.apparatus_schedule_reservations.write().await;
        if let Some(reservation) = reservations.values_mut().find(|reservation| {
            reservation.order_id.trim() == order_id
                && reservation.status == ApparatusScheduleStatus::Paused
                && queue_state::apparatus_titles_match(&reservation.apparatus, from)
        }) {
            reservation.apparatus_id = write.target_apparatus_id.trim().to_string();
            reservation.apparatus = to.to_string();
            reservation.actor = write.record.actor.clone();
        }
    }

    let qolip_codes = session_qolip_codes(&write.session);
    if !qolip_codes.is_empty() {
        let sessions = store.order_run_sessions.read().await;
        for session in sessions.values().filter(|session| {
            session.session_id != write.session.session_id
                && matches!(
                    session.status,
                    OrderRunStatus::Active | OrderRunStatus::Paused
                )
        }) {
            if qolip_codes.iter().any(|code| {
                session_qolip_codes(session)
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(code))
            }) {
                return Err(ProductionMapError::QolipAlreadyInUse);
            }
        }
    }

    let mut maps = store.maps.write().await;
    if !maps.contains_key(order_id) {
        return Err(ProductionMapError::MapNotFound);
    }
    maps.insert(order_id.to_string(), write.updated_map.clone());
    drop(maps);

    store
        .sequences
        .write()
        .await
        .insert(from.to_string(), write.from_sequence.clone());
    store
        .sequences
        .write()
        .await
        .insert(to.to_string(), write.to_sequence.clone());
    store
        .queue_states
        .write()
        .await
        .insert(from.to_string(), write.from_states.clone());
    store
        .queue_states
        .write()
        .await
        .insert(to.to_string(), write.to_states.clone());
    store.order_run_sessions.write().await.insert(
        write.session.session_id.trim().to_string(),
        write.session.clone(),
    );
    store.order_progress_batches.write().await.insert(
        write.progress_batch.batch_id.trim().to_string(),
        write.progress_batch.clone(),
    );
    for update in &write.progress_batch_updates {
        store.order_progress_batches.write().await.insert(
            update.batch_id.trim().to_string(),
            update.clone(),
        );
    }
    if !write.raw_material_assignments.is_empty() {
        let mut assignments = store.material_assignments.write().await;
        for assignment in &write.raw_material_assignments {
            assignments.insert(
                assignment.barcode.trim().to_ascii_uppercase(),
                assignment.clone(),
            );
        }
    }
    store.apparatus_transfers.write().await.insert(
        write.record.idempotency_key.trim().to_string(),
        write.record.clone(),
    );
    Ok(write.record)
}
