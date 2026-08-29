use super::*;

use super::super::queue_state;
use super::runs::session_qolip_codes;
use crate::core::apparatus_standard::ApparatusId;

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
    let mut transfers = store.apparatus_transfers.write().await;
    if let Some(existing) = transfers.get(write.record.idempotency_key.trim()).cloned() {
        if !transfer_record_matches_identity(&existing, &write.record) {
            return Err(ProductionMapError::ApparatusTransferIdempotencyConflict);
        }
        return Ok(existing);
    }

    let order_id = write.record.order_id.trim();
    let from = write.record.from_apparatus.trim();
    let to = write.record.to_apparatus.trim();
    let Ok(from_apparatus_id) = ApparatusId::new(from.to_string()) else {
        return Err(ProductionMapError::MoveNotAllowed);
    };
    let Ok(target_apparatus_id) = ApparatusId::new(write.target_apparatus_id.trim().to_string())
    else {
        return Err(ProductionMapError::MoveNotAllowed);
    };
    if target_apparatus_id.as_str() != to {
        return Err(ProductionMapError::MoveNotAllowed);
    }
    let source_state = store
        .queue_states
        .read()
        .await
        .get(from)
        .and_then(|states| states.get(order_id))
        .map(|state| state.trim().to_string());
    match source_state.as_deref() {
        Some("paused") => {}
        Some("frozen") => return Err(ProductionMapError::OrderFrozen),
        Some("completed") => return Err(ProductionMapError::OrderAlreadyCompleted),
        _ => return Err(ProductionMapError::ApparatusTransferOrderNotPaused),
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
    if super::runs::active_order_run_session(store, to, order_id)
        .await?
        .is_some()
    {
        return Err(ProductionMapError::ApparatusTransferTargetConflict);
    }
    let order_controls = store.order_controls.read().await;
    if let Some(control) = order_controls
        .values()
        .find(|control| control.order_id.trim().eq_ignore_ascii_case(order_id))
    {
        match control.state {
            OrderControlState::Active => {}
            OrderControlState::FreezeRequested => {
                return Err(ProductionMapError::OrderFreezeRequested);
            }
            OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
        }
    }
    drop(order_controls);

    let current_session = store
        .order_run_sessions
        .read()
        .await
        .get(write.record.session_id.trim())
        .cloned()
        .ok_or(ProductionMapError::ApparatusTransferSessionNotFound)?;
    if current_session.order_id.trim() != order_id
        || !queue_state::apparatus_ids_match(&current_session.apparatus, from)
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
        || !queue_state::apparatus_ids_match(&current_batch.apparatus, from)
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

    if !write.raw_material_assignments.is_empty() {
        let assignments = store.material_assignments.read().await;
        for assignment in &write.raw_material_assignments {
            let current = assignments
                .get(&assignment.barcode.trim().to_ascii_uppercase())
                .ok_or(ProductionMapError::RawMaterialAssignmentNotFound)?;
            if current.order_id.trim() != order_id || current.apparatus_id != from_apparatus_id {
                return Err(ProductionMapError::RawMaterialAssignmentNotFound);
            }
        }
    }

    if !store.maps.read().await.contains_key(order_id) {
        return Err(ProductionMapError::MapNotFound);
    }

    let qolip_codes = session_qolip_codes(&write.session);
    if !qolip_codes.is_empty() {
        let sessions = store.order_run_sessions.read().await;
        for session in sessions.values().filter(|session| {
            session.session_id != write.session.session_id
                && matches!(
                    session.status,
                    OrderRunStatus::Active
                        | OrderRunStatus::Paused
                        | OrderRunStatus::Frozen
                        | OrderRunStatus::RollDetached
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

    {
        let mut reservations = store.apparatus_schedule_reservations.write().await;
        if let Some(reservation) = reservations.values_mut().find(|reservation| {
            reservation.order_id.trim() == order_id
                && reservation.status == ApparatusScheduleStatus::Paused
                && reservation.apparatus_id == from_apparatus_id
        }) {
            reservation.apparatus_id = target_apparatus_id;
            reservation.apparatus = to.to_string();
            reservation.actor = write.record.actor.clone();
        }
    }

    let mut maps = store.maps.write().await;
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
        store
            .order_progress_batches
            .write()
            .await
            .insert(update.batch_id.trim().to_string(), update.clone());
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
    transfers.insert(
        write.record.idempotency_key.trim().to_string(),
        write.record.clone(),
    );
    Ok(write.record)
}

fn transfer_record_matches_identity(
    existing: &ProductionMapApparatusTransferRecord,
    requested: &ProductionMapApparatusTransferRecord,
) -> bool {
    existing.order_id.trim() == requested.order_id.trim()
        && existing.from_apparatus.trim() == requested.from_apparatus.trim()
        && existing.to_apparatus.trim() == requested.to_apparatus.trim()
}
