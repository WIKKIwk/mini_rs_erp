use super::*;

use super::progress::{
    actor_display_name, non_empty_or, progress_batch_id, progress_event_id,
    progress_label_item_name, progress_qr_payload, queue_action_str, valid_progress_qty,
};

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

pub(super) fn zero_quantity_event(
    session: &OrderRunSession,
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    actor: &QueueActionActor,
    batch_id: String,
    qr_payload: String,
    payload_json: serde_json::Value,
    now: i64,
) -> OrderProgressEvent {
    OrderProgressEvent {
        event_id: progress_event_id(&session.session_id, order_id, action, now),
        session_id: session.session_id.clone(),
        batch_id,
        apparatus: apparatus.to_string(),
        order_id: order_id.to_string(),
        action,
        produced_qty: 0.0,
        uom: String::new(),
        worker_role: actor.role.trim().to_string(),
        worker_ref: actor.ref_.trim().to_string(),
        worker_display_name: actor.display_name.trim().to_string(),
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

pub(super) struct ProgressOutputIdentity {
    batch_id: String,
    qr_payload: String,
}

pub(super) fn progress_output_identity(
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    now: i64,
    progress: &QueueProgressInput,
    input_progress: &SessionProgressLinks,
) -> ProgressOutputIdentity {
    let output_batch_id_input = if action == queue_state::ApparatusQueueAction::Complete
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
    let output_qr_input = if action == queue_state::ApparatusQueueAction::Complete
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
    }
}

pub(super) fn run_status_for_progress_action(
    action: queue_state::ApparatusQueueAction,
) -> OrderRunStatus {
    match action {
        queue_state::ApparatusQueueAction::Pause => OrderRunStatus::Paused,
        queue_state::ApparatusQueueAction::Complete => OrderRunStatus::Completed,
        _ => OrderRunStatus::Active,
    }
}

fn batch_status_for_progress_action(
    action: queue_state::ApparatusQueueAction,
) -> Result<OrderProgressBatchStatus, ProductionMapError> {
    match action {
        queue_state::ApparatusQueueAction::Pause => Ok(OrderProgressBatchStatus::Paused),
        queue_state::ApparatusQueueAction::Complete => Ok(OrderProgressBatchStatus::Completed),
        _ => Err(ProductionMapError::ProgressInputInvalid),
    }
}

pub(super) fn progress_batch_record(
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    actor: &QueueActionActor,
    session: &OrderRunSession,
    quantity: &ProgressQuantity,
    output_identity: &ProgressOutputIdentity,
    input_progress: &SessionProgressLinks,
    metrics: ProgressMetrics,
    description: &str,
) -> Result<OrderProgressBatch, ProductionMapError> {
    let mut batch = OrderProgressBatch {
        batch_id: output_identity.batch_id.clone(),
        session_id: session.session_id.clone(),
        apparatus: apparatus.to_string(),
        order_id: order_id.to_string(),
        action,
        status: batch_status_for_progress_action(action)?,
        produced_qty: quantity.produced_qty,
        uom: quantity.uom.clone(),
        qr_payload: output_identity.qr_payload.clone(),
        label_item_code: order_id.to_string(),
        label_item_name: progress_label_item_name(order_map, apparatus, action),
        executor_name: actor_display_name(actor),
        worker_role: actor.role.trim().to_string(),
        worker_ref: actor.ref_.trim().to_string(),
        worker_display_name: actor.display_name.trim().to_string(),
        wip_status: OrderProgressBatchWipStatus::Waiting,
        status_detail: OrderProgressBatchStatusDetail::default(),
        current_apparatus: apparatus.to_string(),
        current_apparatus_key: queue_state::apparatus_search_key(apparatus),
        current_location: wip_waiting_location(apparatus),
        next_apparatus: chain::next_work_stage_station(order_map, apparatus).unwrap_or_default(),
        parent_batch_id: input_progress.batch_id.clone(),
        used_by_session_id: String::new(),
        used_by_apparatus: String::new(),
        processed_by_session_id: String::new(),
        processed_by_apparatus: String::new(),
        return_ink_kg: metrics.return_ink_kg,
        lamination_print_leftover_rolls: metrics.lamination_print_leftover_rolls,
        lamination_film_leftover_rolls: metrics.lamination_film_leftover_rolls,
        rezka_bosma_waste: metrics.rezka_bosma_waste,
        rezka_lamination_waste: metrics.rezka_lamination_waste,
        rezka_edge_waste: metrics.rezka_edge_waste,
        total_waste: metrics.total_waste,
        finished_goods_kg: metrics.finished_goods_kg,
        finished_goods_meter: metrics.finished_goods_meter,
        description: description.to_string(),
        payload_json: progress_batch_payload(order_map, apparatus, action, metrics, description),
    };
    sync_wip_payload_fields(&mut batch);
    Ok(batch)
}

pub(super) fn progress_event_record(
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    actor: &QueueActionActor,
    session: &OrderRunSession,
    quantity: ProgressQuantity,
    output_identity: ProgressOutputIdentity,
    metrics: ProgressMetrics,
    description: &str,
    now: i64,
) -> OrderProgressEvent {
    OrderProgressEvent {
        event_id: progress_event_id(&session.session_id, order_id, action, now),
        session_id: session.session_id.clone(),
        batch_id: output_identity.batch_id,
        apparatus: apparatus.to_string(),
        order_id: order_id.to_string(),
        action,
        produced_qty: quantity.produced_qty,
        uom: quantity.uom,
        worker_role: actor.role.trim().to_string(),
        worker_ref: actor.ref_.trim().to_string(),
        worker_display_name: actor.display_name.trim().to_string(),
        qr_payload: output_identity.qr_payload,
        return_ink_kg: metrics.return_ink_kg,
        lamination_print_leftover_rolls: metrics.lamination_print_leftover_rolls,
        lamination_film_leftover_rolls: metrics.lamination_film_leftover_rolls,
        rezka_bosma_waste: metrics.rezka_bosma_waste,
        rezka_lamination_waste: metrics.rezka_lamination_waste,
        rezka_edge_waste: metrics.rezka_edge_waste,
        total_waste: metrics.total_waste,
        finished_goods_kg: metrics.finished_goods_kg,
        finished_goods_meter: metrics.finished_goods_meter,
        description: description.to_string(),
        payload_json: progress_event_payload(action, metrics, description),
    }
}

#[derive(Clone, Copy)]
pub(super) struct ProgressMetrics {
    return_ink_kg: Option<f64>,
    lamination_print_leftover_rolls: Option<f64>,
    lamination_film_leftover_rolls: Option<f64>,
    rezka_bosma_waste: Option<f64>,
    rezka_lamination_waste: Option<f64>,
    rezka_edge_waste: Option<f64>,
    total_waste: Option<f64>,
    finished_goods_kg: Option<f64>,
    finished_goods_meter: Option<f64>,
}

pub(super) fn validated_progress_metrics(
    apparatus: &str,
    action: queue_state::ApparatusQueueAction,
    progress: &QueueProgressInput,
) -> Result<ProgressMetrics, ProductionMapError> {
    let is_complete = action == queue_state::ApparatusQueueAction::Complete;
    let metrics = ProgressMetrics {
        return_ink_kg: if is_complete {
            valid_optional_progress_qty(progress.return_ink_kg)?
        } else {
            None
        },
        lamination_print_leftover_rolls: if is_complete {
            valid_optional_progress_qty(progress.lamination_print_leftover_rolls)?
        } else {
            None
        },
        lamination_film_leftover_rolls: valid_optional_progress_qty(
            progress.lamination_film_leftover_rolls,
        )?,
        rezka_bosma_waste: valid_optional_progress_qty(progress.rezka_bosma_waste)?,
        rezka_lamination_waste: valid_optional_progress_qty(progress.rezka_lamination_waste)?,
        rezka_edge_waste: valid_optional_progress_qty(progress.rezka_edge_waste)?,
        total_waste: valid_optional_progress_qty(progress.total_waste)?,
        finished_goods_kg: valid_optional_progress_qty(progress.finished_goods_kg)?,
        finished_goods_meter: valid_optional_progress_qty(progress.finished_goods_meter)?,
    };
    validate_progress_metrics(apparatus, action, metrics)?;
    Ok(metrics)
}

fn validate_progress_metrics(
    apparatus: &str,
    action: queue_state::ApparatusQueueAction,
    metrics: ProgressMetrics,
) -> Result<(), ProductionMapError> {
    let is_complete = action == queue_state::ApparatusQueueAction::Complete;
    if is_complete
        && pechat::pechat_color_count(apparatus).is_some()
        && !bosma_completion_metrics_are_complete(
            metrics.return_ink_kg,
            metrics.total_waste,
            metrics.finished_goods_kg,
            metrics.finished_goods_meter,
        )
    {
        return Err(ProductionMapError::BosmaCompletionMetricsRequired);
    }
    if is_complete
        && super::apparatus::is_laminatsiya_title(apparatus)
        && !laminatsiya_completion_metrics_are_complete(
            metrics.lamination_print_leftover_rolls,
            metrics.lamination_film_leftover_rolls,
            metrics.total_waste,
            metrics.finished_goods_kg,
            metrics.finished_goods_meter,
        )
    {
        return Err(ProductionMapError::LaminatsiyaCompletionMetricsRequired);
    }
    if super::apparatus::is_rezka_title(apparatus)
        && !rezka_progress_metrics_are_complete(
            metrics.rezka_bosma_waste,
            metrics.rezka_lamination_waste,
            metrics.rezka_edge_waste,
        )
    {
        return Err(ProductionMapError::RezkaProgressMetricsRequired);
    }
    Ok(())
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
        "description": description,
        "input_progress_batch_id": input_progress.batch_id,
        "input_progress_qr_payload": input_progress.qr_payload,
        "input_progress_apparatus": input_progress.apparatus,
    })
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
        "description": description,
    })
}

fn progress_event_payload(
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
        "description": description,
    })
}

pub(super) fn resume_without_progress_payload() -> serde_json::Value {
    serde_json::json!({
        "resumed_without_progress_qr": true,
    })
}

pub(super) fn resumed_batch_payload(actor: &QueueActionActor, now: i64) -> serde_json::Value {
    serde_json::json!({
        "resumed_by": actor,
        "resumed_at_unix": now,
    })
}

pub(super) fn resumed_session_payload(batch: &OrderProgressBatch) -> serde_json::Value {
    serde_json::json!({
        "resumed_batch_id": batch.batch_id,
        "resumed_qr_payload": batch.qr_payload,
    })
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

fn valid_optional_progress_qty(value: Option<f64>) -> Result<Option<f64>, ProductionMapError> {
    match value {
        Some(value) if value.is_finite() && value > 0.0 => Ok(Some(value)),
        Some(_) => Err(ProductionMapError::ProgressInputInvalid),
        None => Ok(None),
    }
}

fn bosma_completion_metrics_are_complete(
    return_ink_kg: Option<f64>,
    total_waste: Option<f64>,
    finished_goods_kg: Option<f64>,
    finished_goods_meter: Option<f64>,
) -> bool {
    return_ink_kg.is_some()
        && total_waste.is_some()
        && finished_goods_kg.is_some()
        && finished_goods_meter.is_some()
}

fn laminatsiya_completion_metrics_are_complete(
    lamination_print_leftover_rolls: Option<f64>,
    lamination_film_leftover_rolls: Option<f64>,
    total_waste: Option<f64>,
    finished_goods_kg: Option<f64>,
    finished_goods_meter: Option<f64>,
) -> bool {
    (lamination_print_leftover_rolls.is_some() || lamination_film_leftover_rolls.is_some())
        && total_waste.is_some()
        && finished_goods_kg.is_some()
        && finished_goods_meter.is_some()
}

fn rezka_progress_metrics_are_complete(
    rezka_bosma_waste: Option<f64>,
    rezka_lamination_waste: Option<f64>,
    rezka_edge_waste: Option<f64>,
) -> bool {
    rezka_bosma_waste.is_some() && rezka_lamination_waste.is_some() && rezka_edge_waste.is_some()
}
