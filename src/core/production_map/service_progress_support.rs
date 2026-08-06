use super::*;

use super::progress::{
    actor_display_name, non_empty_or, progress_batch_id, progress_event_id,
    progress_label_item_name, progress_qr_payload, queue_action_str, valid_progress_qty,
};
use super::service_progress_metrics::ProgressMetrics;

pub(super) fn start_session_payload(
    actor: &QueueActionActor,
    input_progress_batch: Option<&OrderProgressBatch>,
) -> serde_json::Value {
    let (batch_id, qr_payload, apparatus) = input_progress_batch_fields(input_progress_batch);
    serde_json::json!({
        "started_by": actor,
        "input_progress_batch_id": batch_id,
        "input_progress_qr_payload": qr_payload,
        "input_progress_apparatus": apparatus,
    })
}

pub(super) fn start_event_payload(
    input_progress_batch: Option<&OrderProgressBatch>,
) -> serde_json::Value {
    let (batch_id, qr_payload, apparatus) = input_progress_batch_fields(input_progress_batch);
    serde_json::json!({
        "event": "start",
        "input_progress_batch_id": batch_id,
        "input_progress_qr_payload": qr_payload,
        "input_progress_apparatus": apparatus,
    })
}

fn input_progress_batch_fields(
    input_progress_batch: Option<&OrderProgressBatch>,
) -> (&str, &str, &str) {
    input_progress_batch
        .map(|batch| {
            (
                batch.batch_id.as_str(),
                batch.qr_payload.as_str(),
                batch.apparatus.as_str(),
            )
        })
        .unwrap_or_default()
}

#[derive(Clone, Copy)]
pub(super) struct ProgressRecordContext<'a> {
    pub(super) session: &'a OrderRunSession,
    pub(super) apparatus: &'a str,
    pub(super) order_id: &'a str,
    pub(super) action: queue_state::ApparatusQueueAction,
    pub(super) actor: &'a QueueActionActor,
    pub(super) now: i64,
}

pub(super) fn zero_quantity_event(
    context: ProgressRecordContext<'_>,
    batch_id: String,
    qr_payload: String,
    payload_json: serde_json::Value,
) -> OrderProgressEvent {
    OrderProgressEvent {
        event_id: progress_event_id(
            &context.session.session_id,
            context.order_id,
            context.action,
            context.now,
        ),
        session_id: context.session.session_id.clone(),
        batch_id,
        apparatus: context.apparatus.to_string(),
        order_id: context.order_id.to_string(),
        action: context.action,
        produced_qty: 0.0,
        uom: String::new(),
        worker_role: context.actor.role.trim().to_string(),
        worker_ref: context.actor.ref_.trim().to_string(),
        worker_display_name: context.actor.display_name.trim().to_string(),
        qr_payload,
        return_ink_kg: None,
        lamination_print_leftover_rolls: None,
        lamination_film_leftover_rolls: None,
        rezka_bosma_waste: None,
        rezka_lamination_waste: None,
        rezka_edge_waste: None,
        total_waste: None,
        finished_goods_kg: None,
        finished_goods_meter: None,
        diameter: None,
        description: String::new(),
        payload_json,
    }
}

pub(super) struct SessionProgressLinks {
    pub(super) batch_id: String,
    pub(super) qr_payload: String,
    apparatus: String,
}

pub(super) fn session_progress_links(session: &OrderRunSession) -> SessionProgressLinks {
    SessionProgressLinks {
        batch_id: json_string_field(&session.payload_json, "input_progress_batch_id"),
        qr_payload: json_string_field(&session.payload_json, "input_progress_qr_payload"),
        apparatus: json_string_field(&session.payload_json, "input_progress_apparatus"),
    }
}

pub(super) fn progress_links_from_batch(batch: &OrderProgressBatch) -> SessionProgressLinks {
    SessionProgressLinks {
        batch_id: batch.batch_id.clone(),
        qr_payload: batch.qr_payload.clone(),
        apparatus: batch.apparatus.clone(),
    }
}

pub(super) struct ProgressQuantity {
    pub(super) produced_qty: f64,
    pub(super) uom: String,
}

pub(super) fn progress_quantity(
    progress: &QueueProgressInput,
    metrics: ProgressMetrics,
) -> Result<ProgressQuantity, ProductionMapError> {
    let produced_qty = valid_progress_qty(progress.produced_qty.or(metrics.finished_goods_meter))?;
    let uom = if progress.produced_qty.is_none() && metrics.finished_goods_meter.is_some() {
        non_empty_or(&progress.uom, "m")
    } else {
        non_empty_or(&progress.uom, "kg")
    };
    Ok(ProgressQuantity { produced_qty, uom })
}

#[derive(Clone)]
pub(super) struct ProgressOutputIdentity {
    pub(super) batch_id: String,
    pub(super) qr_payload: String,
    pub(super) frame_index: Option<usize>,
    pub(super) frame_count: Option<usize>,
}

pub(super) fn progress_output_identity(
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    now: i64,
    progress: &QueueProgressInput,
    input_progress: &SessionProgressLinks,
) -> ProgressOutputIdentity {
    let input_qr_is_source = matches!(
        action,
        queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::RollComplete
            | queue_state::ApparatusQueueAction::Complete
    );
    let output_batch_id_input = if input_qr_is_source
        && !input_progress.batch_id.trim().is_empty()
        && progress
            .progress_batch_id
            .trim()
            .eq_ignore_ascii_case(input_progress.batch_id.trim())
    {
        ""
    } else {
        progress.progress_batch_id.trim()
    };
    let batch_id = non_empty_or(
        output_batch_id_input,
        &progress_batch_id(apparatus, order_id, action, now),
    );
    let output_qr_input = if input_qr_is_source
        && !input_progress.qr_payload.trim().is_empty()
        && progress
            .qr_payload
            .trim()
            .eq_ignore_ascii_case(input_progress.qr_payload.trim())
    {
        ""
    } else {
        progress.qr_payload.trim()
    };
    let qr_payload = non_empty_or(output_qr_input, &progress_qr_payload(&batch_id));
    ProgressOutputIdentity {
        batch_id,
        qr_payload,
        frame_index: None,
        frame_count: None,
    }
}

pub(super) fn rezka_output_identities(
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    now: i64,
    order_map: &ProductionMapDefinition,
) -> Result<Vec<ProgressOutputIdentity>, ProductionMapError> {
    let frame_count = order_map
        .nodes
        .iter()
        .find(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && progress_apparatus_node_matches(node, apparatus)
        })
        .and_then(|node| node.rezka_kadr_count)
        .filter(|value| *value > 0)
        .ok_or(ProductionMapError::RezkaKadrCountRequired)? as usize;
    let base_id = progress_batch_id(apparatus, order_id, action, now);
    Ok((0..frame_count)
        .map(|index| {
            let batch_id = format!("{base_id}:frame:{}", index + 1);
            ProgressOutputIdentity {
                qr_payload: progress_qr_payload(&batch_id),
                batch_id,
                frame_index: Some(index + 1),
                frame_count: Some(frame_count),
            }
        })
        .collect())
}

pub(super) fn run_status_for_progress_action(
    action: queue_state::ApparatusQueueAction,
) -> OrderRunStatus {
    match action {
        queue_state::ApparatusQueueAction::Pause => OrderRunStatus::Paused,
        queue_state::ApparatusQueueAction::Complete => OrderRunStatus::Completed,
        queue_state::ApparatusQueueAction::RollComplete => OrderRunStatus::Active,
        _ => OrderRunStatus::Active,
    }
}

fn batch_status_for_progress_action(
    action: queue_state::ApparatusQueueAction,
) -> Result<OrderProgressBatchStatus, ProductionMapError> {
    match action {
        queue_state::ApparatusQueueAction::Pause => Ok(OrderProgressBatchStatus::Paused),
        queue_state::ApparatusQueueAction::RollComplete => {
            Ok(OrderProgressBatchStatus::Completed)
        }
        queue_state::ApparatusQueueAction::Complete => Ok(OrderProgressBatchStatus::Completed),
        _ => Err(ProductionMapError::ProgressInputInvalid),
    }
}

pub(super) struct ProgressBatchRecordInput<'a> {
    pub(super) order_map: &'a ProductionMapDefinition,
    pub(super) context: ProgressRecordContext<'a>,
    pub(super) quantity: &'a ProgressQuantity,
    pub(super) output_identity: &'a ProgressOutputIdentity,
    pub(super) input_progress: &'a SessionProgressLinks,
    pub(super) metrics: ProgressMetrics,
    pub(super) description: &'a str,
}

pub(super) fn progress_batch_record(
    input: ProgressBatchRecordInput<'_>,
) -> Result<OrderProgressBatch, ProductionMapError> {
    let context = input.context;
    let mut batch = OrderProgressBatch {
        batch_id: input.output_identity.batch_id.clone(),
        session_id: context.session.session_id.clone(),
        started_at_unix: context.session.started_at_unix,
        completed_at_unix: context.session.updated_at_unix,
        apparatus: context.apparatus.to_string(),
        order_id: context.order_id.to_string(),
        action: context.action,
        status: batch_status_for_progress_action(context.action)?,
        produced_qty: input.quantity.produced_qty,
        uom: input.quantity.uom.clone(),
        qr_payload: input.output_identity.qr_payload.clone(),
        label_item_code: context.order_id.to_string(),
        label_item_name: progress_label_item_name(
            input.order_map,
            context.apparatus,
            context.action,
        ),
        executor_name: actor_display_name(context.actor),
        worker_role: context.actor.role.trim().to_string(),
        worker_ref: context.actor.ref_.trim().to_string(),
        worker_display_name: context.actor.display_name.trim().to_string(),
        wip_status: OrderProgressBatchWipStatus::Waiting,
        status_detail: OrderProgressBatchStatusDetail::default(),
        current_apparatus: context.apparatus.to_string(),
        current_apparatus_key: queue_state::apparatus_search_key(context.apparatus),
        current_location: wip_waiting_location(context.apparatus),
        next_apparatus: chain::next_work_stage_station(input.order_map, context.apparatus)
            .unwrap_or_default(),
        parent_batch_id: input.input_progress.batch_id.clone(),
        used_by_session_id: String::new(),
        used_by_apparatus: String::new(),
        processed_by_session_id: String::new(),
        processed_by_apparatus: String::new(),
        return_ink_kg: input.metrics.return_ink_kg,
        lamination_print_leftover_rolls: input.metrics.lamination_print_leftover_rolls,
        lamination_film_leftover_rolls: input.metrics.lamination_film_leftover_rolls,
        rezka_bosma_waste: input.metrics.rezka_bosma_waste,
        rezka_lamination_waste: input.metrics.rezka_lamination_waste,
        rezka_edge_waste: input.metrics.rezka_edge_waste,
        total_waste: input.metrics.total_waste,
        finished_goods_kg: input.metrics.finished_goods_kg,
        finished_goods_meter: input.metrics.finished_goods_meter,
        diameter: input.metrics.diameter,
        description: input.description.to_string(),
        payload_json: progress_batch_payload(
            input.order_map,
            context.apparatus,
            context.action,
            input.metrics,
            input.description,
        ),
    };
    sync_wip_payload_fields(&mut batch);
    Ok(batch)
}

pub(super) fn apply_rezka_frame_metadata(
    batch: &mut OrderProgressBatch,
    identity: &ProgressOutputIdentity,
    order_map: &ProductionMapDefinition,
    apparatus: &str,
) {
    let (Some(frame_index), Some(frame_count)) = (identity.frame_index, identity.frame_count)
    else {
        return;
    };
    if !batch.payload_json.is_object() {
        batch.payload_json = serde_json::json!({});
    }
    batch.payload_json["rezka_frame_index"] = serde_json::json!(frame_index);
    batch.payload_json["rezka_frame_count"] = serde_json::json!(frame_count);
    batch.payload_json["rezka_output_kind"] = serde_json::json!("frame");
    batch.payload_json["rezka_metrics_owner"] = serde_json::json!(true);
    if let Some(node) = order_map.nodes.iter().find(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && progress_apparatus_node_matches(node, apparatus)
    }) {
        if let Some(kadr_count) = node.rezka_kadr_count {
            batch.payload_json["rezka_kadr_count"] = serde_json::json!(kadr_count);
        }
        if let Some(label_length) = node.rezka_label_length {
            batch.payload_json["rezka_label_length"] = serde_json::json!(label_length);
        }
    }
}

fn progress_apparatus_node_matches(node: &ProductionMapNode, apparatus: &str) -> bool {
    queue_state::apparatus_titles_match(&node.title, apparatus)
        || (!node.alternative_assigned_title.trim().is_empty()
            && queue_state::apparatus_titles_match(
                &node.alternative_assigned_title,
                apparatus,
            ))
}

pub(super) fn clear_rezka_duplicate_metrics(batch: &mut OrderProgressBatch) {
    batch.rezka_bosma_waste = None;
    batch.rezka_lamination_waste = None;
    batch.rezka_edge_waste = None;
    batch.total_waste = None;
    batch.diameter = None;
    if let Some(payload) = batch.payload_json.as_object_mut() {
        payload.remove("diameter");
    }
    batch.payload_json["rezka_metrics_owner"] = serde_json::json!(false);
    sync_wip_payload_fields(batch);
}

pub(super) struct ProgressEventRecordInput<'a> {
    pub(super) context: ProgressRecordContext<'a>,
    pub(super) quantity: ProgressQuantity,
    pub(super) output_identity: ProgressOutputIdentity,
    pub(super) metrics: ProgressMetrics,
    pub(super) description: &'a str,
}

pub(super) fn progress_event_record(input: ProgressEventRecordInput<'_>) -> OrderProgressEvent {
    let context = input.context;
    OrderProgressEvent {
        event_id: progress_event_id(
            &context.session.session_id,
            context.order_id,
            context.action,
            context.now,
        ),
        session_id: context.session.session_id.clone(),
        batch_id: input.output_identity.batch_id,
        apparatus: context.apparatus.to_string(),
        order_id: context.order_id.to_string(),
        action: context.action,
        produced_qty: input.quantity.produced_qty,
        uom: input.quantity.uom,
        worker_role: context.actor.role.trim().to_string(),
        worker_ref: context.actor.ref_.trim().to_string(),
        worker_display_name: context.actor.display_name.trim().to_string(),
        qr_payload: input.output_identity.qr_payload,
        return_ink_kg: input.metrics.return_ink_kg,
        lamination_print_leftover_rolls: input.metrics.lamination_print_leftover_rolls,
        lamination_film_leftover_rolls: input.metrics.lamination_film_leftover_rolls,
        rezka_bosma_waste: input.metrics.rezka_bosma_waste,
        rezka_lamination_waste: input.metrics.rezka_lamination_waste,
        rezka_edge_waste: input.metrics.rezka_edge_waste,
        total_waste: input.metrics.total_waste,
        finished_goods_kg: input.metrics.finished_goods_kg,
        finished_goods_meter: input.metrics.finished_goods_meter,
        diameter: input.metrics.diameter,
        description: input.description.to_string(),
        payload_json: progress_event_payload(context.action, input.metrics, input.description),
    }
}

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
        "finished_goods_meter": metrics.finished_goods_meter,
        "diameter": metrics.diameter,
        "description": description,
        "input_progress_batch_id": input_progress.batch_id,
        "input_progress_qr_payload": input_progress.qr_payload,
        "input_progress_apparatus": input_progress.apparatus,
    })
}

pub(super) fn preserve_qolip_code(
    current: &OrderRunSession,
    mut replacement: serde_json::Value,
) -> serde_json::Value {
    let qolip_code = current
        .payload_json
        .get("qolip_code")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let qolip_codes = current
        .payload_json
        .get("qolip_codes")
        .and_then(serde_json::Value::as_array)
        .filter(|values| !values.is_empty())
        .cloned();
    if qolip_code.is_none() && qolip_codes.is_none() {
        return replacement;
    }
    if !replacement.is_object() {
        replacement = serde_json::json!({});
    }
    if let Some(qolip_code) = qolip_code {
        replacement["qolip_code"] = serde_json::json!(qolip_code);
    }
    if let Some(qolip_codes) = qolip_codes {
        replacement["qolip_codes"] = serde_json::Value::Array(qolip_codes);
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
    payload["worker_handoff"] = serde_json::json!(false);
    payload["roll_removed_from_apparatus"] = serde_json::json!(false);
    preserve_qolip_code(current, payload)
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
    preserve_qolip_code(current, payload)
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
    batch.current_apparatus_key = queue_state::apparatus_search_key(apparatus);
    batch.current_location = apparatus.trim().to_string();
    batch.used_by_session_id = session_id.trim().to_string();
    batch.used_by_apparatus = apparatus.trim().to_string();
    batch.payload_json["wip_in_use_at_unix"] = serde_json::json!(now);
    sync_wip_payload_fields(&mut batch);
    batch
}

pub(super) fn wip_batch_was_consumed_by_producer(batch: &OrderProgressBatch) -> bool {
    batch.action == queue_state::ApparatusQueueAction::Pause
        && batch.wip_status == OrderProgressBatchWipStatus::Processed
        && queue_state::apparatus_titles_match(&batch.processed_by_apparatus, &batch.apparatus)
        && (batch.used_by_apparatus.trim().is_empty()
            || queue_state::apparatus_titles_match(&batch.used_by_apparatus, &batch.apparatus))
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
    batch.current_apparatus_key = queue_state::apparatus_search_key(&batch.apparatus);
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
        || !queue_state::apparatus_titles_match(&batch.apparatus, &recovered_parent.apparatus)
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
    batch.current_apparatus_key = queue_state::apparatus_search_key(&batch.apparatus);
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
    now: i64,
) -> OrderProgressBatch {
    batch.wip_status = OrderProgressBatchWipStatus::Waiting;
    batch.current_apparatus = apparatus.trim().to_string();
    batch.current_apparatus_key = queue_state::apparatus_search_key(apparatus);
    batch.current_location = format!("{} olib tashlandi", apparatus.trim());
    batch.used_by_session_id.clear();
    batch.used_by_apparatus.clear();
    batch.payload_json["worker_handoff"] = serde_json::json!(false);
    batch.payload_json["roll_removed_from_apparatus"] = serde_json::json!(true);
    batch.payload_json["roll_removed_at_unix"] = serde_json::json!(now);
    batch.payload_json["roll_removed_finished_goods_meter"] =
        serde_json::json!(finished_goods_meter);
    batch.payload_json["roll_removed_finished_goods_kg"] = serde_json::json!(finished_goods_kg);
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
    batch.current_apparatus_key = queue_state::apparatus_search_key(apparatus);
    batch.current_location = apparatus.trim().to_string();
    batch.processed_by_session_id = session_id.trim().to_string();
    batch.processed_by_apparatus = apparatus.trim().to_string();
    batch.payload_json["wip_processed_at_unix"] = serde_json::json!(now);
    sync_wip_payload_fields(&mut batch);
    batch
}

pub(super) fn sync_wip_payload_fields(batch: &mut OrderProgressBatch) {
    if !batch.payload_json.is_object() {
        batch.payload_json = serde_json::json!({});
    }
    batch.refresh_status_detail();
    if batch.current_apparatus_key.trim().is_empty() {
        batch.current_apparatus_key = queue_state::apparatus_search_key(&batch.current_apparatus);
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
