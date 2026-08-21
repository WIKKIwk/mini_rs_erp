use super::*;

use std::collections::BTreeSet;

pub(super) async fn active_order_run_session(
    store: &MemoryProductionMapStore,
    apparatus: &str,
    order_id: &str,
) -> Result<Option<OrderRunSession>, ProductionMapError> {
    let apparatus = ApparatusId::new(apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(store
        .order_run_sessions
        .read()
        .await
        .values()
        .find(|session| {
            super::super::types::apparatus_ids_match(&session.apparatus, apparatus.as_str())
                && session.order_id.trim() == order_id.trim()
                && session.status.is_open()
        })
        .cloned())
}

pub(super) async fn active_order_run_session_for_qolip(
    store: &MemoryProductionMapStore,
    qolip_code: &str,
) -> Result<Option<OrderRunSession>, ProductionMapError> {
    let qolip_code = qolip_code.trim();
    if qolip_code.is_empty() {
        return Ok(None);
    }
    Ok(store
        .order_run_sessions
        .read()
        .await
        .values()
        .find(|session| {
            matches!(
                session.status,
                OrderRunStatus::Active | OrderRunStatus::Paused | OrderRunStatus::RollDetached
            ) && !(session.status == OrderRunStatus::Paused
                && session
                    .payload_json
                    .get("requeued_at_tail")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true))
                && session_qolip_codes(session)
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(qolip_code))
        })
        .cloned())
}

pub(super) fn session_qolip_codes(session: &OrderRunSession) -> Vec<String> {
    if session
        .payload_json
        .get("qolip_lock_owner")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return Vec::new();
    }
    QolipLineage::from_payload(&session.payload_json)
        .map(|lineage| lineage.qolip_codes)
        .unwrap_or_default()
}

pub(super) async fn active_order_run_sessions_for_worker(
    store: &MemoryProductionMapStore,
    worker_refs: &[String],
    _worker_display_name: &str,
    limit: usize,
) -> Result<Vec<OrderRunSession>, ProductionMapError> {
    let refs = normalized_worker_refs(worker_refs);
    if refs.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let mut sessions = store
        .order_run_sessions
        .read()
        .await
        .values()
        .filter(|session| {
            matches!(
                session.status,
                OrderRunStatus::Active | OrderRunStatus::Paused | OrderRunStatus::RollDetached
            ) && !(session.status == OrderRunStatus::Paused
                && session
                    .payload_json
                    .get("requeued_at_tail")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true))
                && refs.contains(session.worker_ref.trim())
        })
        .cloned()
        .collect::<Vec<_>>();
    sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at_unix));
    sessions.truncate(limit.min(500));
    Ok(sessions)
}

pub(super) async fn order_run_session(
    store: &MemoryProductionMapStore,
    session_id: &str,
) -> Result<Option<OrderRunSession>, ProductionMapError> {
    Ok(store
        .order_run_sessions
        .read()
        .await
        .get(session_id.trim())
        .cloned())
}

pub(super) async fn order_run_sessions_for_order(
    store: &MemoryProductionMapStore,
    order_id: &str,
) -> Result<Vec<OrderRunSession>, ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Ok(Vec::new());
    }
    let mut sessions = store
        .order_run_sessions
        .read()
        .await
        .values()
        .filter(|session| session.order_id.trim() == order_id)
        .cloned()
        .collect::<Vec<_>>();
    sessions.sort_by(|left, right| {
        left.started_at_unix
            .cmp(&right.started_at_unix)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    Ok(sessions)
}

pub(super) async fn progress_batch(
    store: &MemoryProductionMapStore,
    batch_id: &str,
) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
    Ok(store
        .order_progress_batches
        .read()
        .await
        .get(batch_id.trim())
        .cloned())
}

pub(super) async fn progress_batch_by_qr(
    store: &MemoryProductionMapStore,
    qr_payload: &str,
) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
    let qr_payload = qr_payload.trim();
    Ok(store
        .order_progress_batches
        .read()
        .await
        .values()
        .find(|batch| batch.qr_payload.trim().eq_ignore_ascii_case(qr_payload))
        .cloned())
}

pub(super) async fn progress_batches_for_worker(
    store: &MemoryProductionMapStore,
    worker_refs: &[String],
    _worker_display_name: &str,
    limit: usize,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
    let refs = normalized_worker_refs(worker_refs);
    if refs.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let mut batches = store
        .order_progress_batches
        .read()
        .await
        .values()
        .filter(|batch| refs.contains(batch.worker_ref.trim()))
        .cloned()
        .collect::<Vec<_>>();
    batches.sort_by(|left, right| right.batch_id.cmp(&left.batch_id));
    batches.truncate(limit.min(500));
    Ok(batches)
}

pub(super) async fn progress_batches_for_order(
    store: &MemoryProductionMapStore,
    order_id: &str,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Ok(Vec::new());
    }
    let mut batches = store
        .order_progress_batches
        .read()
        .await
        .values()
        .filter(|batch| batch.order_id.trim() == order_id)
        .cloned()
        .collect::<Vec<_>>();
    batches.sort_by(|left, right| right.batch_id.cmp(&left.batch_id));
    Ok(batches)
}

pub(super) async fn progress_batch_corrections_for_order(
    store: &MemoryProductionMapStore,
    order_id: &str,
) -> Result<Vec<ProgressBatchCorrectionRecord>, ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Ok(Vec::new());
    }
    let batch_ids = store
        .order_progress_batches
        .read()
        .await
        .values()
        .filter(|batch| batch.order_id.trim() == order_id)
        .map(|batch| batch.batch_id.trim().to_string())
        .collect::<BTreeSet<_>>();
    let mut corrections = store
        .progress_batch_corrections
        .read()
        .await
        .iter()
        .filter(|record| batch_ids.contains(record.batch_id.trim()))
        .cloned()
        .collect::<Vec<_>>();
    corrections.sort_by(|left, right| {
        left.created_at_unix
            .cmp(&right.created_at_unix)
            .then_with(|| left.new_revision.cmp(&right.new_revision))
            .then_with(|| left.batch_id.cmp(&right.batch_id))
    });
    Ok(corrections)
}

pub(super) async fn correct_progress_batch(
    store: &MemoryProductionMapStore,
    current: OrderProgressBatch,
    input: ProgressBatchCorrectionInput,
    actor: QueueActionActor,
) -> Result<OrderProgressBatch, ProductionMapError> {
    let mut batches = store.order_progress_batches.write().await;
    let stored = batches
        .get(current.batch_id.trim())
        .cloned()
        .ok_or(ProductionMapError::ProgressBatchNotFound)?;
    if stored.worker_ref.trim() != actor.ref_.trim() {
        return Err(ProductionMapError::ProgressBatchCorrectionForbidden);
    }
    if stored.wip_status != OrderProgressBatchWipStatus::Waiting {
        return Err(ProductionMapError::ProgressBatchCorrectionLocked);
    }
    if stored.revision != input.expected_revision || stored.revision != current.revision {
        return Err(ProductionMapError::ProgressBatchCorrectionConflict);
    }
    let corrected = stored.corrected(&input);
    batches.insert(corrected.batch_id.clone(), corrected.clone());
    drop(batches);
    let created_at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default();
    store
        .progress_batch_corrections
        .write()
        .await
        .push(ProgressBatchCorrectionRecord {
            batch_id: corrected.batch_id.clone(),
            previous_revision: stored.revision,
            new_revision: corrected.revision,
            reason: input.reason.trim().to_string(),
            actor,
            old_values: serde_json::to_value(&stored).unwrap_or(serde_json::Value::Null),
            new_values: serde_json::to_value(&corrected).unwrap_or(serde_json::Value::Null),
            created_at_unix,
        });
    Ok(corrected)
}

pub(super) async fn wip_progress_batches(
    store: &MemoryProductionMapStore,
    query: WipProgressBatchQuery,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
    let WipProgressBatchQuery {
        apparatus,
        next_apparatus,
        current_location,
        status,
        include_processed,
        order_id,
        limit,
    } = query;
    if limit == 0 {
        return Ok(Vec::new());
    }
    let apparatus = apparatus.trim();
    if !apparatus.is_empty() {
        ApparatusId::new(apparatus.to_string()).map_err(|_| ProductionMapError::StoreFailed)?;
    }
    let apparatus_key = super::super::types::canonical_apparatus_key(apparatus);
    let next_apparatus = next_apparatus.trim();
    if !next_apparatus.is_empty() {
        ApparatusId::new(next_apparatus.to_string())
            .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    let current_location = current_location.trim();
    let order_id = order_id.trim();
    let mut batches = store
        .order_progress_batches
        .read()
        .await
        .values()
        .filter(|batch| {
            (apparatus.is_empty()
                || (!apparatus_key.is_empty()
                    && batch.current_apparatus_key.trim() == apparatus_key)
                || super::super::types::apparatus_ids_match(&batch.current_apparatus, apparatus)
                || super::super::types::apparatus_ids_match(&batch.apparatus, apparatus))
                && (current_location.is_empty()
                    || batch.current_location.trim() == current_location)
                && (next_apparatus.is_empty()
                    || super::super::types::stage_ids_match(&batch.next_apparatus, next_apparatus))
                && (order_id.is_empty() || batch.order_id.trim() == order_id)
                && (include_processed
                    || status.map_or(
                        batch.wip_status != OrderProgressBatchWipStatus::Processed,
                        |value| batch.wip_status == value,
                    ))
        })
        .cloned()
        .collect::<Vec<_>>();
    batches.sort_by(|left, right| right.batch_id.cmp(&left.batch_id));
    batches.truncate(limit.min(500));
    Ok(batches)
}

pub(super) async fn put_order_run_session(
    store: &MemoryProductionMapStore,
    session: OrderRunSession,
) -> Result<(), ProductionMapError> {
    validate_session_apparatus(&session)?;
    store
        .order_run_sessions
        .write()
        .await
        .insert(session.session_id.trim().to_string(), session);
    Ok(())
}

fn normalized_worker_refs(worker_refs: &[String]) -> BTreeSet<String> {
    worker_refs
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

pub(super) async fn put_order_progress_event(
    store: &MemoryProductionMapStore,
    event: OrderProgressEvent,
) -> Result<(), ProductionMapError> {
    require_apparatus_id(&event.apparatus)?;
    store.order_progress_events.write().await.push(event);
    Ok(())
}

pub(super) async fn put_order_progress_batch(
    store: &MemoryProductionMapStore,
    batch: OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    validate_progress_batch_apparatus(&batch)?;
    store
        .order_progress_batches
        .write()
        .await
        .insert(batch.batch_id.trim().to_string(), batch);
    Ok(())
}

pub(super) async fn receive_finished_goods_batch(
    store: &MemoryProductionMapStore,
    batch: OrderProgressBatch,
    stock: FinishedGoodsStockEntry,
) -> Result<(), ProductionMapError> {
    validate_progress_batch_apparatus(&batch)?;
    store
        .order_progress_batches
        .write()
        .await
        .insert(batch.batch_id.trim().to_string(), batch);
    store
        .finished_goods_stock
        .write()
        .await
        .insert(stock.id.trim().to_string(), stock);
    Ok(())
}

fn require_apparatus_id(value: &str) -> Result<ApparatusId, ProductionMapError> {
    ApparatusId::new(value.trim().to_string()).map_err(|_| ProductionMapError::StoreFailed)
}

fn validate_session_apparatus(session: &OrderRunSession) -> Result<(), ProductionMapError> {
    require_apparatus_id(&session.apparatus).map(|_| ())
}

fn validate_progress_batch_apparatus(batch: &OrderProgressBatch) -> Result<(), ProductionMapError> {
    require_apparatus_id(&batch.apparatus)?;
    if !batch.current_apparatus_key.trim().is_empty() {
        require_apparatus_id(&batch.current_apparatus_key)?;
    }
    for apparatus in [
        batch.current_apparatus.as_str(),
        batch.next_apparatus.as_str(),
        batch.used_by_apparatus.as_str(),
        batch.processed_by_apparatus.as_str(),
    ] {
        // Finished-goods receipt records the warehouse processing marker in
        // this legacy-shaped field. It is location metadata, not apparatus
        // identity; every other populated value remains canonical-only.
        if !apparatus.trim().is_empty() && !is_warehouse_processing_marker(apparatus) {
            require_apparatus_id(apparatus)?;
        }
    }
    Ok(())
}

fn is_warehouse_processing_marker(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("warehouse:")
}
