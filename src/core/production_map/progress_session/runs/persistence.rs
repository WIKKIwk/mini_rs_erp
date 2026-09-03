use super::*;

pub(in super::super) async fn put_order_run_session(
    store: &MemoryProductionMapStore,
    session: OrderRunSession,
) -> Result<(), ProductionMapError> {
    validate_session_apparatus(&session)?;
    let input_links = input_links_for_persistence(store, &session).await?;
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
        .insert(session_id.clone(), session);
    store
        .order_run_input_links
        .write()
        .await
        .insert(session_id.clone(), input_links);
    store
        .rezka_active_partial_rolls
        .write()
        .await
        .insert(session_id, active_rolls);
    Ok(())
}

async fn input_links_for_persistence(
    store: &MemoryProductionMapStore,
    session: &OrderRunSession,
) -> Result<Vec<OrderRunInputLink>, ProductionMapError> {
    let mut links = order_run_input_links_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    if !links.is_empty() {
        return Ok(links);
    }
    let input_batch_id = payload_string(&session.payload_json, "input_progress_batch_id");
    if input_batch_id.is_empty() {
        return Ok(Vec::new());
    }
    let source_kind_value = payload_string(&session.payload_json, "input_wip_source_kind");
    let source_kind = if source_kind_value.is_empty() {
        legacy_input_source_kind(store, &input_batch_id).await
    } else {
        Some(
            OrderRunInputSourceKind::parse(&source_kind_value)
                .ok_or(ProductionMapError::StoreFailed)?,
        )
    };
    let Some(source_kind) = source_kind else {
        return Ok(Vec::new());
    };
    let processed = session.status == OrderRunStatus::Completed;
    links.push(OrderRunInputLink {
        input_batch_id,
        input_qr_payload: payload_string(&session.payload_json, "input_progress_qr_payload"),
        source_apparatus: payload_string(&session.payload_json, "input_progress_apparatus"),
        source_kind,
        stage_node_id: session.stage_node_id.clone(),
        sequence_no: 1,
        status: if processed {
            OrderRunInputStatus::Processed
        } else {
            OrderRunInputStatus::InUse
        },
        linked_at_unix: session.started_at_unix,
        processed_at_unix: processed.then_some(session.updated_at_unix),
    });
    Ok(links)
}

async fn legacy_input_source_kind(
    store: &MemoryProductionMapStore,
    batch_id: &str,
) -> Option<OrderRunInputSourceKind> {
    let batch_id = batch_id.trim();
    if batch_id.is_empty() {
        return None;
    }
    let is_progress_batch = store
        .order_progress_batches
        .read()
        .await
        .contains_key(batch_id);
    let is_opening_wip = store
        .opening_wip_records
        .read()
        .await
        .values()
        .any(|record| {
            record
                .batches
                .iter()
                .any(|batch| batch.batch_id.trim() == batch_id)
        });
    match (is_progress_batch, is_opening_wip) {
        (true, false) => Some(OrderRunInputSourceKind::ProgressBatch),
        (false, true) => Some(OrderRunInputSourceKind::OpeningWip),
        _ => None,
    }
}

fn payload_string(payload: &serde_json::Value, field: &str) -> String {
    payload
        .get(field)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
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
    let input_links = progress_batch_links_for_persistence(store, &batch).await?;
    let batch_id = batch.batch_id.trim().to_string();
    let order_id = batch.order_id.trim().to_string();
    store
        .order_progress_batches
        .write()
        .await
        .insert(batch_id.clone(), batch);
    store
        .progress_batch_input_links
        .write()
        .await
        .insert(batch_id, input_links);
    if !order_id.is_empty() {
        crate::core::production_map::memory_store::queue::refresh_production_order_lifecycles(store, &[order_id]).await?;
    }
    Ok(())
}

async fn progress_batch_links_for_persistence(
    store: &MemoryProductionMapStore,
    batch: &OrderProgressBatch,
) -> Result<Vec<ProgressBatchInputLink>, ProductionMapError> {
    let mut links = progress_batch_input_links_from_payload(&batch.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    if links.is_empty()
        && let Some(source_kind) = legacy_input_source_kind(store, &batch.parent_batch_id).await
    {
        links.push(ProgressBatchInputLink {
            input_batch_id: batch.parent_batch_id.trim().to_string(),
            input_qr_payload: String::new(),
            source_apparatus: String::new(),
            source_kind,
            sequence_no: 1,
        });
    }
    Ok(links)
}

pub(in super::super) async fn receive_finished_goods_batch(
    store: &MemoryProductionMapStore,
    batch: OrderProgressBatch,
    stock: FinishedGoodsStockEntry,
) -> Result<(), ProductionMapError> {
    validate_progress_batch_apparatus(&batch)?;
    let input_links = progress_batch_links_for_persistence(store, &batch).await?;
    let batch_id = batch.batch_id.trim().to_string();
    let order_id = batch.order_id.trim().to_string();
    store
        .order_progress_batches
        .write()
        .await
        .insert(batch_id.clone(), batch);
    store
        .progress_batch_input_links
        .write()
        .await
        .insert(batch_id, input_links);
    store
        .finished_goods_stock
        .write()
        .await
        .insert(stock.id.trim().to_string(), stock);
    if !order_id.is_empty() {
        crate::core::production_map::memory_store::queue::refresh_production_order_lifecycles(store, &[order_id]).await?;
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
