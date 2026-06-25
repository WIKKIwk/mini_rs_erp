use super::*;

use super::super::queue_state;

pub(super) async fn active_order_run_session(
    store: &MemoryProductionMapStore,
    apparatus: &str,
    order_id: &str,
) -> Result<Option<OrderRunSession>, ProductionMapError> {
    Ok(store
        .order_run_sessions
        .read()
        .await
        .values()
        .find(|session| {
            queue_state::apparatus_titles_match(&session.apparatus, apparatus)
                && session.order_id.trim() == order_id.trim()
                && matches!(
                    session.status,
                    OrderRunStatus::Active | OrderRunStatus::Paused
                )
        })
        .cloned())
}

pub(super) async fn active_order_run_sessions_for_worker(
    store: &MemoryProductionMapStore,
    worker_refs: &[String],
    worker_display_name: &str,
    limit: usize,
) -> Result<Vec<OrderRunSession>, ProductionMapError> {
    let refs = worker_refs
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    let worker_display_name = worker_display_name.trim().to_ascii_lowercase();
    if refs.is_empty() && worker_display_name.is_empty() || limit == 0 {
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
                OrderRunStatus::Active | OrderRunStatus::Paused
            ) && (refs.contains(session.worker_ref.trim())
                || (!worker_display_name.is_empty()
                    && session
                        .worker_display_name
                        .trim()
                        .eq_ignore_ascii_case(&worker_display_name)))
        })
        .cloned()
        .collect::<Vec<_>>();
    sessions.sort_by(|left, right| right.updated_at_unix.cmp(&left.updated_at_unix));
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
    worker_display_name: &str,
    limit: usize,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
    let refs = worker_refs
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    let worker_display_name = worker_display_name.trim().to_ascii_lowercase();
    if refs.is_empty() && worker_display_name.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let mut batches = store
        .order_progress_batches
        .read()
        .await
        .values()
        .filter(|batch| {
            refs.contains(batch.worker_ref.trim())
                || (!worker_display_name.is_empty()
                    && batch
                        .worker_display_name
                        .trim()
                        .eq_ignore_ascii_case(&worker_display_name))
        })
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

pub(super) async fn wip_progress_batches(
    store: &MemoryProductionMapStore,
    apparatus: &str,
    current_location: &str,
    status: Option<OrderProgressBatchWipStatus>,
    order_id: &str,
    limit: usize,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let apparatus = apparatus.trim();
    let apparatus_key = queue_state::apparatus_search_key(apparatus);
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
                || queue_state::apparatus_titles_match(&batch.current_apparatus, apparatus))
                && (current_location.is_empty()
                    || batch.current_location.trim() == current_location)
                && (order_id.is_empty() || batch.order_id.trim() == order_id)
                && status.map_or(
                    batch.wip_status != OrderProgressBatchWipStatus::Processed,
                    |value| batch.wip_status == value,
                )
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
    store
        .order_run_sessions
        .write()
        .await
        .insert(session.session_id.trim().to_string(), session);
    Ok(())
}

pub(super) async fn put_order_progress_event(
    store: &MemoryProductionMapStore,
    event: OrderProgressEvent,
) -> Result<(), ProductionMapError> {
    store.order_progress_events.write().await.push(event);
    Ok(())
}

pub(super) async fn put_order_progress_batch(
    store: &MemoryProductionMapStore,
    batch: OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    store
        .order_progress_batches
        .write()
        .await
        .insert(batch.batch_id.trim().to_string(), batch);
    Ok(())
}
