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
    let stage = chain::work_stage_for_station(
        input.order_map,
        context.apparatus,
        &input.input_progress.stage_node_id,
    )
    .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
    let next_stage = chain::next_work_stage_for_node(input.order_map, &stage.node_id);
    let mut batch = OrderProgressBatch {
        batch_id: input.output_identity.batch_id.clone(),
        revision: 1,
        session_id: context.session.session_id.clone(),
        started_at_unix: progress_batch_started_at(context.session)?,
        completed_at_unix: context.session.updated_at_unix,
        apparatus: context.apparatus.to_string(),
        order_id: context.order_id.to_string(),
        action: context.action,
        status: batch_status_for_progress_action(context.action)?,
        produced_qty: input.quantity.produced_qty,
        uom: input.quantity.uom.clone(),
        qr_payload: input.output_identity.qr_payload.clone(),
        label_item_code: context.order_id.to_string(),
        label_item_name: super::progress::progress_label_item_name_for_stage(
            input.order_map,
            context.apparatus,
            context.action,
            &stage.node_id,
        ),
        executor_name: actor_display_name(context.actor),
        worker_role: context.actor.role.trim().to_string(),
        worker_ref: context.actor.ref_.trim().to_string(),
        worker_display_name: context.actor.display_name.trim().to_string(),
        wip_status: OrderProgressBatchWipStatus::Waiting,
        status_detail: OrderProgressBatchStatusDetail::default(),
        current_apparatus: context.apparatus.to_string(),
        current_location: wip_waiting_location(context.apparatus),
        next_apparatus: next_stage
            .as_ref()
            .and_then(|stage| stage.apparatus_id.clone())
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
    batch.payload_json["stage_node_id"] = serde_json::json!(stage.node_id);
    batch.payload_json["next_stage_node_id"] = serde_json::json!(
        next_stage
            .as_ref()
            .map(|stage| stage.node_id.trim())
            .unwrap_or_default()
    );
    if let Some(gross_qty) = input.frame_gross_qty {
        if !batch.payload_json.is_object() {
            batch.payload_json = serde_json::json!({});
        }
        batch.payload_json["gross_qty"] = serde_json::json!(gross_qty);
    }
    if let Some(contained_kadr_count) = input.output_identity.contained_kadr_count {
        batch.payload_json["contained_kadr_count"] = serde_json::json!(contained_kadr_count);
    }
    let source_input_links = progress_batch_source_input_links(
        context.session,
        input.input_progress,
        input.output_identity,
    )?;
    write_progress_batch_input_links(&mut batch.payload_json, &source_input_links);
    super::layout::apply_output_layout(
        &mut batch, input.order_map, &input.input_progress.layout_lanes,
        !input.input_progress.batch_id.is_empty(), input.output_identity.lane_offset,
        input.output_identity.contained_kadr_count,
    )?;
    batch.refresh_status_detail();
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
    if let Some(output_kind) = identity.rezka_output_kind {
        batch.payload_json["rezka_output_kind"] = serde_json::json!(output_kind);
    }
    if let Some(contained_kadr_count) = identity.contained_kadr_count {
        batch.payload_json["contained_kadr_count"] = serde_json::json!(contained_kadr_count);
    }
    batch.payload_json["rezka_metrics_owner"] = serde_json::json!(true);
    let stage_node_id = json_string_field(&batch.payload_json, "stage_node_id");
    if let Some(node) = order_map.nodes.iter().find(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && ((stage_node_id.is_empty() && progress_apparatus_node_matches(node, apparatus))
                || (!stage_node_id.is_empty() && node.id.trim() == stage_node_id))
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
    let mut event = zero_quantity_event(
        context,
        input.output_identity.batch_id,
        input.output_identity.qr_payload,
        progress_event_payload(context.action, input.metrics, input.description),
    );
    event.produced_qty = input.quantity.produced_qty;
    event.uom = input.quantity.uom;
    event.description = input.description.to_string();
    input.metrics.write_event_fields(&mut event);
    event
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
        "last_action": action.as_str(),
        "last_qty": produced_qty,
        "last_uom": uom,
        "input_progress_batch_id": input_progress.batch_id,
        "input_progress_qr_payload": input_progress.qr_payload,
        "input_progress_apparatus": input_progress.apparatus,
        "input_wip_source_kind": input_progress.source_kind,
    });
    metrics.write_payload_fields(&mut payload, description);
    if !input_progress.layout_lanes.is_empty() {
        payload["layout_lanes"] = serde_json::json!(input_progress.layout_lanes);
    }
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
    if replacement.get("qolip_lock_owner").is_none()
        && let Some(owner) = current.payload_json.get("qolip_lock_owner")
    {
        replacement["qolip_lock_owner"] = owner.clone();
    }
    for field in [
        "qolip_confirmation",
        "qolip_released_on_return",
        "_reactivation_of_session_id",
        "_reactivation_reserved_outputs",
        "reactivation_token",
        START_RESOURCES_PAYLOAD_FIELD,
        INPUT_LINEAGE_PAYLOAD_FIELD,
        REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD,
        "rezka_output_report",
        "rezka_reference_frame_index",
        "rezka_output_cycle",
        "rezka_output_revision",
        "rezka_recorded_kadr_counts",
        "wip_started_at_unix",
        "wip_waiting_for_resume",
        super::temporary_pause::REVISION,
        "_temporary_pause_last_interval",
    ] {
        if let Some(value) = current.payload_json.get(field) {
            replacement[field] = value.clone();
        }
    }
    replacement
}

// Production time belongs to a physical output cycle, not to the lifetime of
// its reusable work session. Freeze, handoff and input merge keep this clock.
pub(super) fn ensure_wip_started_at(session: &mut OrderRunSession) {
    if session.payload_json.get("wip_started_at_unix").is_none() {
        // Pre-cutover reversible resumes are left untouched by the migration
        // so their signed checkpoint remains valid until the next work action.
        let started = if session.payload_json.get("_resume_pause").is_some() {
            session.updated_at_unix
        } else {
            session.started_at_unix
        };
        session.payload_json["wip_started_at_unix"] = serde_json::json!(started);
    }
}

pub(super) fn resume_wip_clock(session: &mut OrderRunSession, now: i64) {
    if session
        .payload_json
        .get("wip_waiting_for_resume")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        session.payload_json["wip_started_at_unix"] = serde_json::json!(now);
        session.payload_json["wip_waiting_for_resume"] = serde_json::json!(false);
    }
}

pub(super) fn progress_batch_started_at(
    session: &OrderRunSession,
) -> Result<i64, ProductionMapError> {
    // Frames in one generation start together; recording one frame freezes its
    // completion without restarting the still-mounted frames. Legacy partial
    // roll metadata was sometimes initialized at Merge and is not a start event.
    let started = session
        .payload_json
        .get("wip_started_at_unix")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(session.started_at_unix);
    if started < 0 || started > session.updated_at_unix {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    Ok(started)
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
    if matches!(
        action,
        queue_state::ApparatusQueueAction::Pause | queue_state::ApparatusQueueAction::DetachRoll
    ) {
        payload["wip_waiting_for_resume"] = serde_json::json!(true);
    } else if action == queue_state::ApparatusQueueAction::RollComplete {
        payload["wip_started_at_unix"] = serde_json::json!(now);
        payload["wip_waiting_for_resume"] = serde_json::json!(false);
    }
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
