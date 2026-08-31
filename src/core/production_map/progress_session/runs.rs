use super::*;

use std::collections::{BTreeMap, BTreeSet};

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

pub(super) async fn active_order_run_sessions_for_orders(
    store: &MemoryProductionMapStore,
    order_ids: &[String],
) -> Result<BTreeMap<String, Vec<OrderRunSession>>, ProductionMapError> {
    let requested = order_ids
        .iter()
        .map(|order_id| order_id.trim())
        .filter(|order_id| !order_id.is_empty())
        .collect::<BTreeSet<_>>();
    if requested.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut sessions_by_order = BTreeMap::new();
    for session in store.order_run_sessions.read().await.values() {
        let order_id = session.order_id.trim();
        if session.status.is_open() && requested.contains(order_id) {
            sessions_by_order
                .entry(order_id.to_string())
                .or_insert_with(Vec::new)
                .push(session.clone());
        }
    }
    Ok(sessions_by_order)
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
                OrderRunStatus::Active
                    | OrderRunStatus::Paused
                    | OrderRunStatus::Frozen
                    | OrderRunStatus::RollDetached
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
                OrderRunStatus::Active
                    | OrderRunStatus::Paused
                    | OrderRunStatus::Frozen
                    | OrderRunStatus::RollDetached
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
        stage_node_id: payload_string(&session.payload_json, "stage_node_id"),
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
    let input_links = progress_batch_links_for_persistence(store, &batch).await?;
    let batch_id = batch.batch_id.trim().to_string();
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

pub(super) async fn receive_finished_goods_batch(
    store: &MemoryProductionMapStore,
    batch: OrderProgressBatch,
    stock: FinishedGoodsStockEntry,
) -> Result<(), ProductionMapError> {
    validate_progress_batch_apparatus(&batch)?;
    let input_links = progress_batch_links_for_persistence(store, &batch).await?;
    let batch_id = batch.batch_id.trim().to_string();
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

#[cfg(test)]
mod merge_lineage_memory_store_tests {
    use super::*;

    fn input_link(
        batch_id: &str,
        sequence_no: u32,
        status: OrderRunInputStatus,
    ) -> OrderRunInputLink {
        OrderRunInputLink {
            input_batch_id: batch_id.to_string(),
            input_qr_payload: format!("qr:{batch_id}"),
            source_apparatus: "apparatus:catalog:print-001".to_string(),
            source_kind: OrderRunInputSourceKind::ProgressBatch,
            stage_node_id: "rezka".to_string(),
            sequence_no,
            status,
            linked_at_unix: 10,
            processed_at_unix: (status == OrderRunInputStatus::Processed).then_some(20),
        }
    }

    #[tokio::test]
    async fn memory_store_persists_session_partial_roll_and_output_source_lineage() {
        let store = MemoryProductionMapStore::new();
        let input_links = vec![
            input_link("wip-a", 1, OrderRunInputStatus::Processed),
            input_link("wip-b", 2, OrderRunInputStatus::InUse),
        ];
        let active_rolls = vec![RezkaActivePartialRoll {
            slot_index: 1,
            generation: 1,
            contained_kadr_count: 1,
            status: RezkaPartialRollStatus::Active,
            source_input_batch_ids: vec!["wip-a".to_string(), "wip-b".to_string()],
            started_at_unix: 10,
            updated_at_unix: 20,
        }];
        let mut session_payload = serde_json::json!({});
        write_order_run_input_links(&mut session_payload, &input_links);
        write_rezka_active_partial_rolls(&mut session_payload, &active_rolls);
        let session = OrderRunSession {
            session_id: "run-rezka-1".to_string(),
            apparatus: "apparatus:default:asset-010".to_string(),
            order_id: "order-1".to_string(),
            status: OrderRunStatus::Active,
            worker_role: "aparatchi".to_string(),
            worker_ref: "worker-1".to_string(),
            worker_display_name: "Worker".to_string(),
            started_at_unix: 10,
            updated_at_unix: 20,
            payload_json: session_payload,
        };
        put_order_run_session(&store, session)
            .await
            .expect("session");

        assert_eq!(
            store.order_run_input_links("run-rezka-1").await,
            input_links
        );
        assert_eq!(
            store.rezka_active_partial_rolls("run-rezka-1").await,
            active_rolls
        );

        let output_links = vec![
            ProgressBatchInputLink {
                input_batch_id: "wip-a".to_string(),
                input_qr_payload: "qr:wip-a".to_string(),
                source_apparatus: "apparatus:catalog:print-001".to_string(),
                source_kind: OrderRunInputSourceKind::ProgressBatch,
                sequence_no: 1,
            },
            ProgressBatchInputLink {
                input_batch_id: "wip-b".to_string(),
                input_qr_payload: "qr:wip-b".to_string(),
                source_apparatus: "apparatus:catalog:print-001".to_string(),
                source_kind: OrderRunInputSourceKind::ProgressBatch,
                sequence_no: 2,
            },
        ];
        let mut batch_payload = serde_json::json!({});
        write_progress_batch_input_links(&mut batch_payload, &output_links);
        let batch: OrderProgressBatch = serde_json::from_value(serde_json::json!({
            "batch_id": "rezka-output-1",
            "session_id": "run-rezka-1",
            "started_at_unix": 10,
            "completed_at_unix": 20,
            "apparatus": "apparatus:default:asset-010",
            "order_id": "order-1",
            "action": "roll_complete",
            "status": "completed",
            "produced_qty": 100.0,
            "uom": "m",
            "qr_payload": "qr:rezka-output-1",
            "label_item_code": "order-1",
            "label_item_name": "Rezka output",
            "executor_name": "Worker",
            "worker_role": "aparatchi",
            "worker_ref": "worker-1",
            "worker_display_name": "Worker",
            "wip_status": "waiting",
            "parent_batch_id": "wip-b",
            "payload_json": batch_payload,
        }))
        .expect("output batch");
        put_order_progress_batch(&store, batch)
            .await
            .expect("output batch persistence");

        assert_eq!(
            store.progress_batch_input_links("rezka-output-1").await,
            output_links
        );
    }
}
