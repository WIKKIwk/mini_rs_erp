
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
    metrics.write_event_fields(&mut event);
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
    let mut payload = serde_json::json!({
        "last_action": queue_action_str(action),
        "last_qty": produced_qty,
        "last_uom": uom,
        "input_progress_batch_id": input_progress.batch_id,
        "input_progress_qr_payload": input_progress.qr_payload,
        "input_progress_apparatus": input_progress.apparatus,
        "input_wip_source_kind": input_progress.source_kind,
    });
    metrics.write_payload_fields(&mut payload, description);
    if let Some(contained_kadr_count) = input_progress.contained_kadr_count {
        payload["contained_kadr_count"] = serde_json::json!(contained_kadr_count);
    }
    payload
}

pub(super) fn preserve_qolip_lineage(
    current: &OrderRunSession,
    mut replacement: serde_json::Value,
) -> serde_json::Value {
    if let Some(lineage) = QolipLineage::from_payload(&current.payload_json) {
        lineage.write_to_payload(&mut replacement);
    }
    if !replacement.is_object() {
        replacement = serde_json::json!({});
    }
    for field in [
        INPUT_LINEAGE_PAYLOAD_FIELD,
        REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD,
    ] {
        if let Some(value) = current.payload_json.get(field) {
            replacement[field] = value.clone();
        }
    }
    replacement
}

pub(super) fn initialize_rezka_active_partial_rolls(
    payload: &mut serde_json::Value,
    output_kadr_counts: &[usize],
    now: i64,
) -> Result<(), ProductionMapError> {
    let source_input_batch_ids = order_run_input_links_from_payload(payload)
        .map_err(|_| ProductionMapError::ProgressInputInvalid)?
        .into_iter()
        .filter(|link| link.status == OrderRunInputStatus::InUse)
        .map(|link| link.input_batch_id)
        .collect::<Vec<_>>();
    let rolls = output_kadr_counts
        .iter()
        .enumerate()
        .map(|(index, contained_kadr_count)| {
            Ok(RezkaActivePartialRoll {
                slot_index: u32::try_from(index + 1)
                    .map_err(|_| ProductionMapError::InvalidRezkaFrameGroups)?,
                generation: 1,
                contained_kadr_count: u32::try_from(*contained_kadr_count)
                    .map_err(|_| ProductionMapError::InvalidRezkaFrameGroups)?,
                status: RezkaPartialRollStatus::Active,
                source_input_batch_ids: source_input_batch_ids.clone(),
                started_at_unix: now,
                updated_at_unix: now,
            })
        })
        .collect::<Result<Vec<_>, ProductionMapError>>()?;
    write_rezka_active_partial_rolls(payload, &rolls);
    Ok(())
}

fn progress_batch_source_input_links(
    session: &OrderRunSession,
    input_progress: &SessionProgressLinks,
    output_identity: &ProgressOutputIdentity,
) -> Result<Vec<ProgressBatchInputLink>, ProductionMapError> {
    let input_links = order_run_input_links_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
    let active_rolls = rezka_active_partial_rolls_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
    if !rezka_merge_state_is_consistent(&input_links, &active_rolls) {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    let selected_source_ids = output_identity
        .frame_index
        .and_then(|frame_index| {
            u32::try_from(frame_index).ok().and_then(|slot_index| {
                active_rolls
                    .iter()
                    .find(|roll| roll.slot_index == slot_index)
            })
        })
        .map(|roll| roll.source_input_batch_ids.as_slice());

    let mut links = input_links
        .into_iter()
        .filter(|link| {
            selected_source_ids.is_none_or(|source_ids| {
                source_ids
                    .iter()
                    .any(|batch_id| batch_id.trim() == link.input_batch_id.trim())
            })
        })
        .map(|link| ProgressBatchInputLink {
            input_batch_id: link.input_batch_id,
            input_qr_payload: link.input_qr_payload,
            source_apparatus: link.source_apparatus,
            source_kind: link.source_kind,
            sequence_no: link.sequence_no,
        })
        .collect::<Vec<_>>();
    links.sort_by_key(|link| link.sequence_no);

    if links.is_empty()
        && !input_progress.batch_id.trim().is_empty()
        && let Some(source_kind) = OrderRunInputSourceKind::parse(&input_progress.source_kind)
    {
        links.push(ProgressBatchInputLink {
            input_batch_id: input_progress.batch_id.trim().to_string(),
            input_qr_payload: input_progress.qr_payload.trim().to_string(),
            source_apparatus: input_progress.apparatus.trim().to_string(),
            source_kind,
            sequence_no: 1,
        });
    }
    Ok(links)
}

pub(super) fn apply_output_boundary_to_session_payload(
    payload: &mut serde_json::Value,
    action: queue_state::ApparatusQueueAction,
    current_input_batch_id: &str,
    now: i64,
) -> Result<(), ProductionMapError> {
    if action == queue_state::ApparatusQueueAction::Complete {
        let mut links = order_run_input_links_from_payload(payload)
            .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
        if let Some(link) = links.iter_mut().find(|link| {
            link.input_batch_id.trim() == current_input_batch_id.trim()
                && link.status == OrderRunInputStatus::InUse
        }) {
            link.status = OrderRunInputStatus::Processed;
            link.processed_at_unix = Some(now);
        }
        write_order_run_input_links(payload, &links);
        write_rezka_active_partial_rolls(payload, &[]);
        return Ok(());
    }
    if action != queue_state::ApparatusQueueAction::RollComplete {
        return Ok(());
    }

    let mut rolls = rezka_active_partial_rolls_from_payload(payload)
        .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
    for roll in &mut rolls {
        roll.generation = roll
            .generation
            .checked_add(1)
            .ok_or(ProductionMapError::ProgressInputInvalid)?;
        roll.source_input_batch_ids = if current_input_batch_id.trim().is_empty() {
            Vec::new()
        } else {
            vec![current_input_batch_id.trim().to_string()]
        };
        roll.started_at_unix = now;
        roll.updated_at_unix = now;
    }
    write_rezka_active_partial_rolls(payload, &rolls);
    Ok(())
}

fn progress_batch_payload(
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    action: queue_state::ApparatusQueueAction,
    metrics: ProgressMetrics,
    description: &str,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "order_title": order_map.title.trim(),
        "customer_name": order_map.customer_name.trim(),
        "apparatus": apparatus,
        "action": queue_action_str(action),
    });
    metrics.write_payload_fields(&mut payload, description);
    payload
}

pub(super) fn progress_event_payload(
    action: queue_state::ApparatusQueueAction,
    metrics: ProgressMetrics,
    description: &str,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "event": queue_action_str(action),
    });
    metrics.write_payload_fields(&mut payload, description);
    payload
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
    batch.refresh_status_detail();
    batch
}

pub(super) fn wip_batch_was_consumed_by_producer(batch: &OrderProgressBatch) -> bool {
    batch.action.creates_resumable_output()
        && batch.wip_status == OrderProgressBatchWipStatus::Processed
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
    batch.refresh_status_detail();
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
    batch.refresh_status_detail();
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
    batch.refresh_status_detail();
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
    batch.refresh_status_detail();
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

pub(super) fn json_string_field(payload: &serde_json::Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn json_positive_usize_field(payload: &serde_json::Value, key: &str) -> Option<usize> {
    payload
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
}
