use super::*;

pub(in super::super) async fn put_order_run_session(
    store: &MemoryProductionMapStore,
    session: OrderRunSession,
) -> Result<(), ProductionMapError> {
    validate_session_apparatus(&session)?;
    let input_links = order_run_input_links_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let payload_active_rolls = rezka_active_partial_rolls_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let active_rolls = if session.status.is_open() {
        payload_active_rolls
    } else {
        if !payload_active_rolls.is_empty() {
            return Err(ProductionMapError::StoreFailed);
        }
        Vec::new()
    };
    if !rezka_merge_state_is_consistent(&input_links, &active_rolls) {
        return Err(ProductionMapError::StoreFailed);
    }
    let session_id = session.session_id.trim().to_string();
    store
        .order_run_sessions
        .write()
        .await
        .insert(session_id, session);
    Ok(())
}

pub(in super::super) async fn put_order_progress_event(
    store: &MemoryProductionMapStore,
    event: OrderProgressEvent,
) -> Result<(), ProductionMapError> {
    require_apparatus_id(&event.apparatus)?;
    store.order_progress_events.write().await.push(event);
    Ok(())
}

pub(in super::super) async fn put_order_progress_batch(
    store: &MemoryProductionMapStore,
    batch: OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    validate_progress_batch_apparatus(&batch)?;
    progress_batch_input_links_from_payload(&batch.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let batch_id = batch.batch_id.trim().to_string();
    let order_id = batch.order_id.trim().to_string();
    store
        .order_progress_batches
        .write()
        .await
        .insert(batch_id, batch);
    if !order_id.is_empty() {
        crate::core::production_map::memory_store::queue::refresh_production_order_lifecycles(
            store,
            &[order_id],
        )
        .await?;
    }
    Ok(())
}

pub(in super::super) async fn receive_finished_goods_batch(
    store: &MemoryProductionMapStore,
    batch: OrderProgressBatch,
    stock: FinishedGoodsStockEntry,
) -> Result<(), ProductionMapError> {
    validate_progress_batch_apparatus(&batch)?;
    progress_batch_input_links_from_payload(&batch.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let batch_id = batch.batch_id.trim().to_string();
    let order_id = batch.order_id.trim().to_string();
    store
        .order_progress_batches
        .write()
        .await
        .insert(batch_id, batch);
    store
        .finished_goods_stock
        .write()
        .await
        .insert(stock.id.trim().to_string(), stock);
    if !order_id.is_empty() {
        crate::core::production_map::memory_store::queue::refresh_production_order_lifecycles(
            store,
            &[order_id],
        )
        .await?;
    }
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
