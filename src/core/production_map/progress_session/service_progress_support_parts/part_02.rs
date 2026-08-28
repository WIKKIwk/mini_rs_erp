
pub(super) fn progress_metrics_event(
    context: ProgressRecordContext<'_>,
    batch_id: String,
    qr_payload: String,
    metrics: ProgressMetrics,
    description: &str,
    event_name: &str,
) -> OrderProgressEvent {
    let mut event = zero_quantity_event(
        context,
        batch_id,
        qr_payload,
        progress_event_payload(context.action, metrics, description),
    );
    event.payload_json["event"] = serde_json::json!(event_name);
    event.lamination_print_leftover_rolls = metrics.lamination_print_leftover_rolls;
    event.lamination_film_leftover_rolls = metrics.lamination_film_leftover_rolls;
    event.total_waste = metrics.total_waste;
    event.finished_goods_kg = metrics.finished_goods_kg;
    event.bobina_kg = metrics.bobina_kg;
    event.finished_goods_meter = metrics.finished_goods_meter;
    event.diameter = metrics.diameter;
    event.description = description.to_string();
    event
}

pub(super) fn progress_session_payload(
    action: queue_state::ApparatusQueueAction,
    produced_qty: f64,
    uom: &str,
    metrics: ProgressMetrics,
    description: &str,
    input_progress: &SessionProgressLinks,
) -> serde_json::Value {
    serde_json::json!({
        "last_action": queue_action_str(action),
        "last_qty": produced_qty,
        "last_uom": uom,
        "return_ink_kg": metrics.return_ink_kg,
        "lamination_print_leftover_rolls": metrics.lamination_print_leftover_rolls,
        "lamination_film_leftover_rolls": metrics.lamination_film_leftover_rolls,
        "rezka_bosma_waste": metrics.rezka_bosma_waste,
        "rezka_lamination_waste": metrics.rezka_lamination_waste,
        "rezka_edge_waste": metrics.rezka_edge_waste,
        "total_waste": metrics.total_waste,
        "total_waste_uom": "kg",
        "finished_goods_kg": metrics.finished_goods_kg,
        "bobina_kg": metrics.bobina_kg,
        "finished_goods_meter": metrics.finished_goods_meter,
        "diameter": metrics.diameter,
        "description": description,
        "input_progress_batch_id": input_progress.batch_id,
        "input_progress_qr_payload": input_progress.qr_payload,
        "input_progress_apparatus": input_progress.apparatus,
        "input_wip_source_kind": input_progress.source_kind,
    })
}

pub(super) fn preserve_qolip_lineage(
    current: &OrderRunSession,
    mut replacement: serde_json::Value,
) -> serde_json::Value {
    if let Some(lineage) = QolipLineage::from_payload(&current.payload_json) {
        lineage.write_to_payload(&mut replacement);
    }
    replacement
}

fn progress_batch_payload(
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    action: queue_state::ApparatusQueueAction,
    metrics: ProgressMetrics,
    description: &str,
) -> serde_json::Value {
    serde_json::json!({
        "order_title": order_map.title.trim(),
        "customer_name": order_map.customer_name.trim(),
        "apparatus": apparatus,
        "action": queue_action_str(action),
        "return_ink_kg": metrics.return_ink_kg,
        "lamination_print_leftover_rolls": metrics.lamination_print_leftover_rolls,
        "lamination_film_leftover_rolls": metrics.lamination_film_leftover_rolls,
        "rezka_bosma_waste": metrics.rezka_bosma_waste,
        "rezka_lamination_waste": metrics.rezka_lamination_waste,
        "rezka_edge_waste": metrics.rezka_edge_waste,
        "total_waste": metrics.total_waste,
        "total_waste_uom": "kg",
        "finished_goods_kg": metrics.finished_goods_kg,
        "bobina_kg": metrics.bobina_kg,
        "finished_goods_meter": metrics.finished_goods_meter,
        "diameter": metrics.diameter,
        "description": description,
    })
}

pub(super) fn progress_event_payload(
    action: queue_state::ApparatusQueueAction,
    metrics: ProgressMetrics,
    description: &str,
) -> serde_json::Value {
    serde_json::json!({
        "event": queue_action_str(action),
        "return_ink_kg": metrics.return_ink_kg,
        "lamination_print_leftover_rolls": metrics.lamination_print_leftover_rolls,
        "lamination_film_leftover_rolls": metrics.lamination_film_leftover_rolls,
        "rezka_bosma_waste": metrics.rezka_bosma_waste,
        "rezka_lamination_waste": metrics.rezka_lamination_waste,
        "rezka_edge_waste": metrics.rezka_edge_waste,
        "total_waste": metrics.total_waste,
        "total_waste_uom": "kg",
        "finished_goods_kg": metrics.finished_goods_kg,
        "bobina_kg": metrics.bobina_kg,
        "finished_goods_meter": metrics.finished_goods_meter,
        "diameter": metrics.diameter,
        "description": description,
    })
}

pub(super) fn worker_handoff_session_payload(
    metrics: ProgressMetrics,
    description: &str,
    input_progress: &SessionProgressLinks,
) -> serde_json::Value {
    let mut payload = progress_session_payload(
        queue_state::ApparatusQueueAction::Pause,
        0.0,
        "",
        metrics,
        description,
        input_progress,
    );
    payload["worker_handoff"] = serde_json::json!(true);
    payload["roll_removed_from_apparatus"] = serde_json::json!(false);
    payload
}

pub(super) fn removed_roll_session_payload(
    metrics: ProgressMetrics,
    description: &str,
    input_progress: &SessionProgressLinks,
) -> serde_json::Value {
    let mut payload = progress_session_payload(
        queue_state::ApparatusQueueAction::Pause,
        0.0,
        "",
        metrics,
        description,
        input_progress,
    );
    payload["worker_handoff"] = serde_json::json!(false);
    payload["roll_removed_from_apparatus"] = serde_json::json!(true);
    payload
}

pub(super) fn resumed_handoff_session_payload(
    current: &OrderRunSession,
    input_progress: &SessionProgressLinks,
) -> serde_json::Value {
    let mut payload = current.payload_json.clone();
    if !payload.is_object() {
        payload = serde_json::json!({});
    }
    payload["resumed_batch_id"] = serde_json::json!(input_progress.batch_id);
    payload["resumed_qr_payload"] = serde_json::json!(input_progress.qr_payload);
    payload["input_progress_batch_id"] = serde_json::json!(input_progress.batch_id);
    payload["input_progress_qr_payload"] = serde_json::json!(input_progress.qr_payload);
    payload["input_progress_apparatus"] = serde_json::json!(input_progress.apparatus);
    payload["input_wip_source_kind"] = serde_json::json!(input_progress.source_kind);
    payload["worker_handoff"] = serde_json::json!(false);
    payload["roll_removed_from_apparatus"] = serde_json::json!(false);
    preserve_qolip_lineage(current, payload)
}

pub(super) fn resumed_batch_payload(
    batch: &OrderProgressBatch,
    actor: &QueueActionActor,
    now: i64,
) -> serde_json::Value {
    let mut payload = batch.payload_json.clone();
    if !payload.is_object() {
        payload = serde_json::json!({});
    }
    payload["resumed_by"] = serde_json::json!(actor);
    payload["resumed_at_unix"] = serde_json::json!(now);
    payload
}

pub(super) fn resumed_session_payload(
    current: &OrderRunSession,
    output_batch: &OrderProgressBatch,
    resumed_without_progress_qr: bool,
) -> serde_json::Value {
    let mut payload = current.payload_json.clone();
    if !payload.is_object() {
        payload = serde_json::json!({});
    }
    payload["resumed_batch_id"] = serde_json::json!(output_batch.batch_id);
    payload["resumed_qr_payload"] = serde_json::json!(output_batch.qr_payload);
    payload["resumed_without_progress_qr"] = serde_json::json!(resumed_without_progress_qr);
    preserve_qolip_lineage(current, payload)
}

pub(super) fn resume_event_payload() -> serde_json::Value {
    serde_json::json!({"event": "resume"})
}

pub(super) fn wip_batch_in_use(
    mut batch: OrderProgressBatch,
    apparatus: &str,
    session_id: &str,
    now: i64,
) -> OrderProgressBatch {
    clear_wip_processing_fields(&mut batch);
    batch.wip_status = OrderProgressBatchWipStatus::InUse;
    batch.current_apparatus = apparatus.trim().to_string();
    batch.current_apparatus_key = super::types::canonical_apparatus_key(apparatus);
    batch.current_location = apparatus.trim().to_string();
    batch.used_by_session_id = session_id.trim().to_string();
    batch.used_by_apparatus = apparatus.trim().to_string();
    batch.payload_json["wip_in_use_at_unix"] = serde_json::json!(now);
    sync_wip_payload_fields(&mut batch);
    batch
}

pub(super) fn wip_batch_was_consumed_by_producer(batch: &OrderProgressBatch) -> bool {
    matches!(
        batch.action,
        queue_state::ApparatusQueueAction::Pause | queue_state::ApparatusQueueAction::DetachRoll
    ) && batch.wip_status == OrderProgressBatchWipStatus::Processed
        && super::types::apparatus_ids_match(&batch.processed_by_apparatus, &batch.apparatus)
        && (batch.used_by_apparatus.trim().is_empty()
            || super::types::apparatus_ids_match(&batch.used_by_apparatus, &batch.apparatus))
        && (batch.processed_by_session_id.trim().is_empty()
            || batch.processed_by_session_id.trim() == batch.session_id.trim())
}

pub(super) fn restore_self_consumed_wip(batch: &mut OrderProgressBatch) -> bool {
    if !wip_batch_was_consumed_by_producer(batch) {
        return false;
    }
    clear_wip_usage_fields(batch);
    batch.wip_status = OrderProgressBatchWipStatus::Waiting;
    batch.current_apparatus = batch.apparatus.trim().to_string();
    batch.current_apparatus_key = super::types::canonical_apparatus_key(&batch.apparatus);
    batch.current_location = wip_waiting_location(&batch.apparatus);
    if !batch.payload_json.is_object() {
        batch.payload_json = serde_json::json!({});
    }
    batch.payload_json["recovered_self_consumed_wip"] = serde_json::json!(true);
    sync_wip_payload_fields(batch);
    true
}

pub(super) fn normalize_self_consumed_wip_history(batches: &mut [OrderProgressBatch]) {
    let recovered_parents = batches
        .iter()
        .filter(|batch| wip_batch_was_consumed_by_producer(batch))
        .cloned()
        .collect::<Vec<_>>();
    for batch in batches.iter_mut() {
        for parent in &recovered_parents {
            if repair_self_consumed_sibling_lineage(batch, parent) {
                break;
            }
        }
    }
    for batch in batches {
        restore_self_consumed_wip(batch);
    }
}

pub(super) fn repair_self_consumed_sibling_lineage(
    batch: &mut OrderProgressBatch,
    recovered_parent: &OrderProgressBatch,
) -> bool {
    if batch.parent_batch_id.trim() != recovered_parent.batch_id.trim()
        || batch.session_id.trim() != recovered_parent.session_id.trim()
        || !super::types::apparatus_ids_match(&batch.apparatus, &recovered_parent.apparatus)
    {
        return false;
    }
    batch.parent_batch_id.clear();
    if !batch.payload_json.is_object() {
        batch.payload_json = serde_json::json!({});
    }
    batch.payload_json["recovered_sibling_lineage"] = serde_json::json!(true);
    sync_wip_payload_fields(batch);
    true
}

pub(super) fn restore_misbound_output_wip(
    mut batch: OrderProgressBatch,
    now: i64,
) -> OrderProgressBatch {
    clear_wip_usage_fields(&mut batch);
    batch.wip_status = OrderProgressBatchWipStatus::Waiting;
    batch.current_apparatus = batch.apparatus.trim().to_string();
    batch.current_apparatus_key = super::types::canonical_apparatus_key(&batch.apparatus);
    batch.current_location = wip_waiting_location(&batch.apparatus);
    if !batch.payload_json.is_object() {
        batch.payload_json = serde_json::json!({});
    }
    batch.payload_json["recovered_output_input_confusion"] = serde_json::json!(true);
    batch.payload_json["recovered_at_unix"] = serde_json::json!(now);
    sync_wip_payload_fields(&mut batch);
    batch
}

pub(super) fn wip_batch_worker_handoff(
    mut batch: OrderProgressBatch,
    apparatus: &str,
    session_id: &str,
    now: i64,
) -> OrderProgressBatch {
    batch = wip_batch_in_use(batch, apparatus, session_id, now);
    batch.payload_json["worker_handoff"] = serde_json::json!(true);
    batch.payload_json["roll_removed_from_apparatus"] = serde_json::json!(false);
    batch.payload_json["worker_handoff_at_unix"] = serde_json::json!(now);
    sync_wip_payload_fields(&mut batch);
    batch
}

pub(super) fn wip_batch_claimed_after_handoff(
    mut batch: OrderProgressBatch,
    apparatus: &str,
    session_id: &str,
    now: i64,
) -> OrderProgressBatch {
    batch = wip_batch_in_use(batch, apparatus, session_id, now);
    batch.payload_json["worker_handoff"] = serde_json::json!(false);
    batch.payload_json["roll_removed_from_apparatus"] = serde_json::json!(false);
    batch.payload_json["roll_claimed_after_handoff_at_unix"] = serde_json::json!(now);
    sync_wip_payload_fields(&mut batch);
    batch
}

pub(super) fn wip_batch_removed_from_apparatus(
    mut batch: OrderProgressBatch,
    apparatus: &str,
    finished_goods_meter: f64,
    finished_goods_kg: f64,
    bobina_kg: f64,
    now: i64,
) -> OrderProgressBatch {
    batch.wip_status = OrderProgressBatchWipStatus::Waiting;
    batch.current_apparatus = apparatus.trim().to_string();
    batch.current_apparatus_key = super::types::canonical_apparatus_key(apparatus);
    batch.current_location = format!("{} olib tashlandi", apparatus.trim());
    batch.used_by_session_id.clear();
    batch.used_by_apparatus.clear();
    batch.payload_json["worker_handoff"] = serde_json::json!(false);
    batch.payload_json["roll_removed_from_apparatus"] = serde_json::json!(true);
    batch.payload_json["roll_removed_at_unix"] = serde_json::json!(now);
    batch.payload_json["roll_removed_finished_goods_meter"] =
        serde_json::json!(finished_goods_meter);
    batch.payload_json["roll_removed_finished_goods_kg"] = serde_json::json!(finished_goods_kg);
    batch.payload_json["roll_removed_bobina_kg"] = serde_json::json!(bobina_kg);
    batch.bobina_kg = Some(bobina_kg);
    sync_wip_payload_fields(&mut batch);
    batch
}

pub(super) fn wip_batch_processed(
    mut batch: OrderProgressBatch,
    apparatus: &str,
    session_id: &str,
    now: i64,
) -> OrderProgressBatch {
    batch.wip_status = OrderProgressBatchWipStatus::Processed;
    batch.current_apparatus = apparatus.trim().to_string();
    batch.current_apparatus_key = super::types::canonical_apparatus_key(apparatus);
    batch.current_location = apparatus.trim().to_string();
    batch.processed_by_session_id = session_id.trim().to_string();
    batch.processed_by_apparatus = apparatus.trim().to_string();
    batch.payload_json["wip_processed_at_unix"] = serde_json::json!(now);
    sync_wip_payload_fields(&mut batch);
    batch
}

pub(super) fn opening_wip_batch_in_use(
    mut batch: OpeningWipBatch,
    apparatus: &str,
    session_id: &str,
    now: i64,
) -> OpeningWipBatch {
    batch.wip_status = OpeningWipBatchStatus::InUse;
    batch.used_by_session_id = session_id.trim().to_string();
    batch.used_by_apparatus = apparatus.trim().to_string();
    batch.processed_by_session_id.clear();
    batch.processed_by_apparatus.clear();
    batch.updated_at_unix = now;
    batch
}

pub(super) fn opening_wip_batch_processed(
    mut batch: OpeningWipBatch,
    apparatus: &str,
    session_id: &str,
    now: i64,
) -> OpeningWipBatch {
    batch.wip_status = OpeningWipBatchStatus::Processed;
    batch.processed_by_session_id = session_id.trim().to_string();
    batch.processed_by_apparatus = apparatus.trim().to_string();
    batch.updated_at_unix = now;
    batch
}

pub(super) fn opening_wip_batch_waiting(
    mut batch: OpeningWipBatch,
    now: i64,
) -> OpeningWipBatch {
    batch.wip_status = OpeningWipBatchStatus::Waiting;
    batch.used_by_session_id.clear();
    batch.used_by_apparatus.clear();
    batch.processed_by_session_id.clear();
    batch.processed_by_apparatus.clear();
    batch.updated_at_unix = now;
    batch
}

pub(super) fn sync_wip_payload_fields(batch: &mut OrderProgressBatch) {
    if !batch.payload_json.is_object() {
        batch.payload_json = serde_json::json!({});
    }
    batch.refresh_status_detail();
    if batch.current_apparatus_key.trim().is_empty() {
        batch.current_apparatus_key =
            super::types::canonical_apparatus_key(&batch.current_apparatus);
    }
    if let Some(lineage) = QolipLineage::from_payload(&batch.payload_json) {
        lineage.write_to_payload(&mut batch.payload_json);
    }
    batch.payload_json["status_detail"] = serde_json::json!(batch.status_detail);
    batch.payload_json["wip_status"] = serde_json::json!(batch.wip_status.as_str());
    batch.payload_json["current_apparatus"] = serde_json::json!(batch.current_apparatus);
    batch.payload_json["current_apparatus_key"] = serde_json::json!(batch.current_apparatus_key);
    batch.payload_json["current_location"] = serde_json::json!(batch.current_location);
    batch.payload_json["next_apparatus"] = serde_json::json!(batch.next_apparatus);
    batch.payload_json["parent_batch_id"] = serde_json::json!(batch.parent_batch_id);
    batch.payload_json["used_by_session_id"] = serde_json::json!(batch.used_by_session_id);
    batch.payload_json["used_by_apparatus"] = serde_json::json!(batch.used_by_apparatus);
    batch.payload_json["used_by_order_id"] = serde_json::json!(batch.order_id);
    batch.payload_json["processed_by_session_id"] =
        serde_json::json!(batch.processed_by_session_id);
    batch.payload_json["processed_by_apparatus"] = serde_json::json!(batch.processed_by_apparatus);
    batch.payload_json["from_apparatus"] = serde_json::json!(batch.apparatus);
}

fn clear_wip_processing_fields(batch: &mut OrderProgressBatch) {
    batch.processed_by_session_id.clear();
    batch.processed_by_apparatus.clear();
    if let Some(payload) = batch.payload_json.as_object_mut() {
        payload.remove("wip_processed_at_unix");
    }
}

fn clear_wip_usage_fields(batch: &mut OrderProgressBatch) {
    batch.used_by_session_id.clear();
    batch.used_by_apparatus.clear();
    clear_wip_processing_fields(batch);
    if let Some(payload) = batch.payload_json.as_object_mut() {
        payload.remove("wip_in_use_at_unix");
    }
}

fn wip_waiting_location(apparatus: &str) -> String {
    let apparatus = apparatus.trim();
    if apparatus.is_empty() {
        String::new()
    } else {
        format!("{apparatus} chiqim")
    }
}

fn json_string_field(payload: &serde_json::Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim()
        .to_string()
}
