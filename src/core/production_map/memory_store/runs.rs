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
