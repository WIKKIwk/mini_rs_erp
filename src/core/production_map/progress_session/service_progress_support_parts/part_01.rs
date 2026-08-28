
pub(super) fn start_session_payload(
    actor: &QueueActionActor,
    input_progress: &SessionProgressLinks,
    input_progress_batch: Option<&OrderProgressBatch>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "started_by": actor,
        "input_progress_batch_id": input_progress.batch_id,
        "input_progress_qr_payload": input_progress.qr_payload,
        "input_progress_apparatus": input_progress.apparatus,
        "input_wip_source_kind": input_progress.source_kind,
    });
    if let Some(lineage) = input_progress_batch.and_then(qolip_lineage_from_batch) {
        lineage.write_to_payload(&mut payload);
    }
    payload
}

pub(super) fn start_event_payload(
    input_progress: &SessionProgressLinks,
    input_progress_batch: Option<&OrderProgressBatch>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "event": "start",
        "input_progress_batch_id": input_progress.batch_id,
        "input_progress_qr_payload": input_progress.qr_payload,
        "input_progress_apparatus": input_progress.apparatus,
        "input_wip_source_kind": input_progress.source_kind,
    });
    if let Some(lineage) = input_progress_batch.and_then(qolip_lineage_from_batch) {
        lineage.write_to_payload(&mut payload);
    }
    payload
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
        bobina_kg: None,
        finished_goods_meter: None,
        diameter: None,
        description: String::new(),
        payload_json,
    }
}

#[derive(Clone, Default)]
pub(super) struct SessionProgressLinks {
    pub(super) batch_id: String,
    pub(super) qr_payload: String,
    pub(super) apparatus: String,
    pub(super) source_kind: String,
}

pub(super) fn session_progress_links(session: &OrderRunSession) -> SessionProgressLinks {
    SessionProgressLinks {
        batch_id: json_string_field(&session.payload_json, "input_progress_batch_id"),
        qr_payload: json_string_field(&session.payload_json, "input_progress_qr_payload"),
        apparatus: json_string_field(&session.payload_json, "input_progress_apparatus"),
        source_kind: json_string_field(&session.payload_json, "input_wip_source_kind"),
    }
}

pub(super) fn progress_links_from_batch(batch: &OrderProgressBatch) -> SessionProgressLinks {
    SessionProgressLinks {
        batch_id: batch.batch_id.clone(),
        qr_payload: batch.qr_payload.clone(),
        apparatus: batch.apparatus.clone(),
        source_kind: "progress_batch".to_string(),
    }
}

pub(super) fn progress_links_from_opening_wip(
    record: &OpeningWipBatchRecord,
) -> SessionProgressLinks {
    SessionProgressLinks {
        batch_id: record.batch.batch_id.clone(),
        qr_payload: record.batch.qr_payload.clone(),
        apparatus: if record.intake.source_apparatus.trim().is_empty() {
            record.intake.source_operation.clone()
        } else {
            record.intake.source_apparatus.clone()
        },
        source_kind: "opening_wip".to_string(),
    }
}

#[derive(Clone)]
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
            | queue_state::ApparatusQueueAction::DetachRoll
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
        queue_state::ApparatusQueueAction::Freeze => OrderRunStatus::Frozen,
        queue_state::ApparatusQueueAction::DetachRoll => OrderRunStatus::RollDetached,
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
        queue_state::ApparatusQueueAction::DetachRoll => Ok(OrderProgressBatchStatus::RollDetached),
        queue_state::ApparatusQueueAction::RollComplete => Ok(OrderProgressBatchStatus::Completed),
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
    pub(super) frame_gross_qty: Option<f64>,
    pub(super) description: &'a str,
}

pub(super) fn progress_batch_record(
    input: ProgressBatchRecordInput<'_>,
) -> Result<OrderProgressBatch, ProductionMapError> {
    let context = input.context;
    let mut batch = OrderProgressBatch {
        batch_id: input.output_identity.batch_id.clone(),
        revision: 1,
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
        current_apparatus_key: super::types::canonical_apparatus_key(context.apparatus),
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
        bobina_kg: input.metrics.bobina_kg,
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
    if let Some(lineage) = QolipLineage::from_payload(&context.session.payload_json) {
        lineage.write_to_payload(&mut batch.payload_json);
    }
    if let Some(gross_qty) = input.frame_gross_qty {
        if !batch.payload_json.is_object() {
            batch.payload_json = serde_json::json!({});
        }
        batch.payload_json["gross_qty"] = serde_json::json!(gross_qty);
    }
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
    super::types::apparatus_ids_match(&node.apparatus_id, apparatus)
        || (!node.alternative_assigned_apparatus_id.trim().is_empty()
            && super::types::apparatus_ids_match(
                &node.alternative_assigned_apparatus_id,
                apparatus,
            ))
}

pub(super) fn clear_rezka_duplicate_metrics(batch: &mut OrderProgressBatch) {
    batch.rezka_bosma_waste = None;
    batch.rezka_lamination_waste = None;
    batch.rezka_edge_waste = None;
    batch.total_waste = None;
    batch.bobina_kg = None;
    batch.diameter = None;
    if let Some(payload) = batch.payload_json.as_object_mut() {
        payload.remove("bobina_kg");
        payload.remove("diameter");
        payload.remove("rezka_bosma_waste");
        payload.remove("rezka_lamination_waste");
        payload.remove("rezka_edge_waste");
        payload.remove("total_waste");
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
        bobina_kg: input.metrics.bobina_kg,
        finished_goods_meter: input.metrics.finished_goods_meter,
        diameter: input.metrics.diameter,
        description: input.description.to_string(),
        payload_json: progress_event_payload(context.action, input.metrics, input.description),
    }
}
