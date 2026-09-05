use super::*;

use super::progress::{
    actor_display_name, non_empty_or, progress_batch_id, progress_event_id,
    progress_qr_payload, qolip_lineage_from_batch,
    valid_progress_qty, QolipLineage,
};
use super::service_progress_metrics::ProgressMetrics;


pub(super) fn start_session_payload(
    actor: &QueueActionActor,
    input_progress: &SessionProgressLinks,
    input_progress_batch: Option<&OrderProgressBatch>,
    stage_node_id: &str,
    now: i64,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "started_by": actor,
        "input_progress_batch_id": input_progress.batch_id,
        "input_progress_qr_payload": input_progress.qr_payload,
        "input_progress_apparatus": input_progress.apparatus,
        "input_wip_source_kind": input_progress.source_kind,
    });
    if let Some(contained_kadr_count) = input_progress.contained_kadr_count {
        payload["contained_kadr_count"] = serde_json::json!(contained_kadr_count);
    }
    if let Some(lineage) = input_progress_batch.and_then(qolip_lineage_from_batch) {
        lineage.write_to_payload(&mut payload);
    }
    if !input_progress.batch_id.trim().is_empty()
        && let Some(source_kind) = OrderRunInputSourceKind::parse(&input_progress.source_kind)
    {
        write_order_run_input_links(
            &mut payload,
            &[OrderRunInputLink {
                input_batch_id: input_progress.batch_id.trim().to_string(),
                input_qr_payload: input_progress.qr_payload.trim().to_string(),
                source_apparatus: input_progress.apparatus.trim().to_string(),
                source_kind,
                stage_node_id: stage_node_id.trim().to_string(),
                sequence_no: 1,
                status: OrderRunInputStatus::InUse,
                linked_at_unix: now,
                processed_at_unix: None,
            }],
        );
    }
    payload
}

pub(super) fn start_event_payload(
    input_progress: &SessionProgressLinks,
    input_progress_batch: Option<&OrderProgressBatch>,
    stage_node_id: &str,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "event": "start",
        "input_progress_batch_id": input_progress.batch_id,
        "input_progress_qr_payload": input_progress.qr_payload,
        "input_progress_apparatus": input_progress.apparatus,
        "input_wip_source_kind": input_progress.source_kind,
        "stage_node_id": stage_node_id.trim(),
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
    pub(super) stage_node_id: String,
    pub(super) contained_kadr_count: Option<usize>,
}

pub(super) fn session_progress_links(session: &OrderRunSession) -> SessionProgressLinks {
    SessionProgressLinks {
        batch_id: json_string_field(&session.payload_json, "input_progress_batch_id"),
        qr_payload: json_string_field(&session.payload_json, "input_progress_qr_payload"),
        apparatus: json_string_field(&session.payload_json, "input_progress_apparatus"),
        source_kind: json_string_field(&session.payload_json, "input_wip_source_kind"),
        stage_node_id: session.stage_node_id.trim().to_string(),
        contained_kadr_count: json_positive_usize_field(
            &session.payload_json,
            "contained_kadr_count",
        ),
    }
}

pub(super) fn progress_links_from_batch(batch: &OrderProgressBatch) -> SessionProgressLinks {
    SessionProgressLinks {
        batch_id: batch.batch_id.clone(),
        qr_payload: batch.qr_payload.clone(),
        apparatus: batch.apparatus.clone(),
        source_kind: "progress_batch".to_string(),
        stage_node_id: json_string_field(&batch.payload_json, "next_stage_node_id"),
        contained_kadr_count: json_positive_usize_field(
            &batch.payload_json,
            "contained_kadr_count",
        ),
    }
}

pub(super) fn progress_links_from_opening_wip(
    record: &OpeningWipBatchRecord,
    target_stage_node_id: &str,
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
        stage_node_id: target_stage_node_id.trim().to_string(),
        contained_kadr_count: None,
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
    pub(super) contained_kadr_count: Option<usize>,
    pub(super) rezka_output_kind: Option<&'static str>,
}

pub(super) fn progress_output_identity(
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    progress: &QueueProgressInput,
    input_progress: &SessionProgressLinks,
) -> ProgressOutputIdentity {
    let input_qr_is_source = action.records_progress_output();
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
        &progress_batch_id(apparatus, order_id, action),
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
        contained_kadr_count: input_progress.contained_kadr_count,
        rezka_output_kind: None,
    }
}

pub(super) fn rezka_output_identities(
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    order_map: &ProductionMapDefinition,
    stage_node_id: &str,
    input_contained_kadr_count: Option<usize>,
) -> Result<Vec<ProgressOutputIdentity>, ProductionMapError> {
    let output_kadr_counts = rezka_output_kadr_counts(
        order_map,
        apparatus,
        stage_node_id,
        input_contained_kadr_count,
    )?;
    rezka_output_identities_from_kadr_counts(
        apparatus,
        order_id,
        action,
        order_map,
        stage_node_id,
        &output_kadr_counts,
    )
}

pub(super) fn rezka_output_identities_from_kadr_counts(
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    order_map: &ProductionMapDefinition,
    stage_node_id: &str,
    output_kadr_counts: &[usize],
) -> Result<Vec<ProgressOutputIdentity>, ProductionMapError> {
    if output_kadr_counts.is_empty() || output_kadr_counts.contains(&0) {
        return Err(ProductionMapError::InvalidRezkaFrameGroups);
    }
    let output_count = output_kadr_counts.len();
    let is_final = chain::is_final_work_stage_node(order_map, stage_node_id);
    let base_id = progress_batch_id(apparatus, order_id, action);
    Ok(output_kadr_counts
        .iter()
        .copied()
        .enumerate()
        .map(|(index, contained_kadr_count)| {
            let batch_id = format!("{base_id}:frame:{}", index + 1);
            ProgressOutputIdentity {
                qr_payload: progress_qr_payload(&batch_id),
                batch_id,
                frame_index: Some(index + 1),
                frame_count: Some(output_count),
                contained_kadr_count: Some(contained_kadr_count),
                rezka_output_kind: Some(if is_final { "frame" } else { "grouped_roll" }),
            }
        })
        .collect())
}

pub(crate) fn rezka_output_kadr_counts(
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    stage_node_id: &str,
    input_contained_kadr_count: Option<usize>,
) -> Result<Vec<usize>, ProductionMapError> {
    let stage = chain::work_stage_for_station(order_map, apparatus, stage_node_id)
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
    let node = order_map
        .nodes
        .iter()
        .find(|node| node.id.trim() == stage.node_id.trim())
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
    let total_kadr_count = node
        .rezka_kadr_count
        .filter(|value| *value > 0)
        .ok_or(ProductionMapError::RezkaKadrCountRequired)? as usize;
    if chain::is_final_work_stage_node(order_map, &stage.node_id) {
        let count = input_contained_kadr_count.unwrap_or(total_kadr_count);
        if count == 0 {
            return Err(ProductionMapError::RezkaKadrCountRequired);
        }
        return Ok(vec![1; count]);
    }
    if node.rezka_frame_groups.is_empty() {
        return Ok(vec![1; total_kadr_count]);
    }
    node.rezka_frame_groups
        .iter()
        .map(|value| {
            usize::try_from(*value)
                .ok()
                .filter(|value| *value > 0)
                .ok_or(ProductionMapError::InvalidRezkaFrameGroups)
        })
        .collect()
}

#[cfg(test)]
mod rezka_output_kadr_count_tests {
    use super::*;

    fn repeated_rezka_map() -> ProductionMapDefinition {
        serde_json::from_value(serde_json::json!({
            "id": "zakaz-rezka-groups",
            "product_code": "REZKA-GROUPS",
            "title": "Rezka groups",
            "nodes": [
                {"id": "start", "kind": "start", "title": "Start"},
                {"id": "rezka_before_lamination", "kind": "apparatus", "title": "Rezka", "apparatus_id": "apparatus:default:asset-010", "rezka_kadr_count": 3, "rezka_frame_groups": [1, 2]},
                {"id": "lamination", "kind": "apparatus", "title": "Laminatsiya", "apparatus_id": "apparatus:catalog:lam-001"},
                {"id": "rezka_final", "kind": "apparatus", "title": "Rezka", "apparatus_id": "apparatus:default:asset-010", "rezka_kadr_count": 3},
                {"id": "end", "kind": "end", "title": "End"}
            ],
            "edges": [
                {"from": "start", "to": "rezka_before_lamination"},
                {"from": "rezka_before_lamination", "to": "lamination"},
                {"from": "lamination", "to": "rezka_final"},
                {"from": "rezka_final", "to": "end"}
            ]
        }))
        .expect("repeated Rezka map")
    }

    #[test]
    fn intermediate_rezka_uses_configured_groups_and_final_uses_input_metadata() {
        let map = repeated_rezka_map();

        assert_eq!(
            rezka_output_kadr_counts(
                &map,
                "apparatus:default:asset-010",
                "rezka_before_lamination",
                None,
            ),
            Ok(vec![1, 2])
        );
        assert_eq!(
            rezka_output_kadr_counts(
                &map,
                "apparatus:default:asset-010",
                "rezka_final",
                Some(2),
            ),
            Ok(vec![1, 1])
        );
        assert_eq!(
            rezka_output_kadr_counts(
                &map,
                "apparatus:default:asset-010",
                "rezka_final",
                None,
            ),
            Ok(vec![1, 1, 1])
        );
    }
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
        "rezka_output_report",
        "rezka_output_cycle",
        "rezka_output_revision",
        "rezka_recorded_kadr_counts",
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
        "action": action.as_str(),
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
        "event": action.as_str(),
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
