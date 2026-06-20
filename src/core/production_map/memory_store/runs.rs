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
