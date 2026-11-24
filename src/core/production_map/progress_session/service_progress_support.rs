use super::*;

use super::progress::{
    actor_display_name, non_empty_or, progress_batch_id, progress_event_id,
    progress_qr_payload, qolip_lineage_from_batch,
    valid_progress_qty, QolipLineage,
};
use super::service_progress_metrics::ProgressMetrics;

/// Preserve the WIP ownership guard while reporting why a matching QR is blocked.
pub(super) fn progress_batch_usage_error(
    batch: &OrderProgressBatch,
    map: &ProductionMapDefinition,
) -> ProductionMapError {
    let owner = match batch.wip_status {
        OrderProgressBatchWipStatus::Processed => non_empty_or(
            &batch.processed_by_apparatus,
            &batch.used_by_apparatus,
        ),
        _ => batch.used_by_apparatus.clone(),
    };
    let owner = non_empty_or(&owner, &batch.current_apparatus);
    let apparatus_name = map.nodes.iter()
        .find(|node| node.kind == ProductionMapNodeKind::Apparatus
            && (node.apparatus_id.trim() == owner.trim() || node.title.trim() == owner.trim()))
        .map(|node| node.title.trim().to_string())
        .filter(|name| !name.starts_with("apparatus:"))
        .unwrap_or_default();
    match batch.wip_status {
        OrderProgressBatchWipStatus::Processed =>
            ProductionMapError::ProgressBatchAlreadyUsed { apparatus_name },
        OrderProgressBatchWipStatus::InUse =>
            ProductionMapError::ProgressBatchInUse { apparatus_name },
        OrderProgressBatchWipStatus::Waiting | OrderProgressBatchWipStatus::Peremotka =>
            ProductionMapError::ProgressBatchNotAccepted,
    }
}


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
        "wip_started_at_unix": now,
        "wip_waiting_for_resume": false,
    });
    if !input_progress.layout_lanes.is_empty() {
        payload["layout_lanes"] = serde_json::json!(input_progress.layout_lanes);
    }
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
    pub(super) layout_lanes: Vec<crate::core::order_layout::OrderLane>,
    pub(super) batch_id: String,
    pub(super) qr_payload: String,
    pub(super) apparatus: String,
    pub(super) source_kind: String,
    pub(super) stage_node_id: String,
    pub(super) contained_kadr_count: Option<usize>,
}

pub(super) fn session_progress_links(session: &OrderRunSession) -> SessionProgressLinks {
    SessionProgressLinks {
        layout_lanes: super::layout::payload_lanes(&session.payload_json),
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

pub(crate) fn lamination_input_was_released(session: &OrderRunSession) -> bool {
    matches!(session.status, OrderRunStatus::Paused | OrderRunStatus::RollDetached)
        && session_progress_links(session).batch_id.trim().is_empty()
        && (session.payload_json.get("lamination_astatka_shift_closed")
            .and_then(serde_json::Value::as_bool) == Some(true)
            || session.payload_json.get("layer_remainder_detached_input_batch_id")
                .and_then(serde_json::Value::as_str).is_some_and(|id| !id.trim().is_empty()))
}

pub(super) fn progress_links_from_batch(batch: &OrderProgressBatch) -> SessionProgressLinks {
    SessionProgressLinks {
        layout_lanes: super::layout::payload_lanes(&batch.payload_json),
        batch_id: batch.batch_id.clone(),
        qr_payload: batch.qr_payload.clone(),
        apparatus: batch.apparatus.clone(),
        source_kind: "progress_batch".to_string(),
        stage_node_id: super::service_queue_support::progress_batch_next_stage_node_id(batch).to_string(),
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
        layout_lanes: Vec::new(),
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
    pub(super) lane_offset: Option<usize>,
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
        lane_offset: None,
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
    let mut lane_offset = 0;
    let is_final = chain::is_final_work_stage_node(order_map, stage_node_id);
    let base_id = progress_batch_id(apparatus, order_id, action);
    Ok(output_kadr_counts
        .iter()
        .copied()
        .enumerate()
        .map(|(index, contained_kadr_count)| {
            let batch_id = format!("{base_id}:frame:{}", index + 1);
            let offset = lane_offset;
            lane_offset += contained_kadr_count;
            ProgressOutputIdentity {
                lane_offset: Some(offset),
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
    rezka_output_kadr_counts_for_node(
        node,
        input_contained_kadr_count,
        chain::is_final_work_stage_node(order_map, &stage.node_id),
    )
}

/// Snapshot callers already own this immutable map's topology projection.
/// Reuse its stage/adjacency witnesses instead of traversing the entire graph
/// several more times for every Rezka control on every queue refresh.
pub(crate) fn rezka_output_kadr_counts_projected(
    projection: &chain::ProjectionMap<'_>,
    apparatus: &str,
    stage_node_id: &str,
    input_contained_kadr_count: Option<usize>,
) -> Result<Vec<usize>, ProductionMapError> {
    let stage = projection
        .work_stage(apparatus, stage_node_id)
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
    let node = projection.map().nodes.iter()
        .find(|node| node.id.trim() == stage.node_id.trim())
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
    rezka_output_kadr_counts_for_node(
        node,
        input_contained_kadr_count,
        stage.apparatus_id.is_some() && projection.next_stages(&stage.node_id).is_empty(),
    )
}

fn rezka_output_kadr_counts_for_node(
    node: &ProductionMapNode,
    input_contained_kadr_count: Option<usize>,
    is_final: bool,
) -> Result<Vec<usize>, ProductionMapError> {
    let total_kadr_count = node
        .rezka_kadr_count
        .filter(|value| *value > 0)
        .ok_or(ProductionMapError::RezkaKadrCountRequired)? as usize;
    if is_final {
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
#[path = "rezka_output_kadr_count_tests.rs"]
mod rezka_output_kadr_count_tests;

/// A detached output may already be downstream; closing must not reclaim it.
pub(super) fn bosma_closing_output<'a>(
    session: &OrderRunSession,
    batches: &'a [OrderProgressBatch],
) -> Option<&'a OrderProgressBatch> {
    if session.status != OrderRunStatus::RollDetached {
        return None;
    }
    let anchor = session.payload_json.get("last_detached_output_batch_id")
        .and_then(serde_json::Value::as_str);
    batches.iter().filter(|batch| {
        batch.session_id == session.session_id
            && batch.order_id == session.order_id
            && batch.apparatus == session.apparatus
            && batch.action == queue_state::ApparatusQueueAction::DetachRoll
            && batch.status == OrderProgressBatchStatus::RollDetached
            && anchor.is_none_or(|id| id == batch.batch_id)
    }).max_by(|a, b| (a.completed_at_unix, &a.batch_id).cmp(&(b.completed_at_unix, &b.batch_id)))
}

pub(super) fn validate_bosma_closing_metrics(progress: &QueueProgressInput) -> Result<(), ProductionMapError> {
    let nonnegative = |value: f64| value.is_finite() && value >= 0.0;
    if !progress.total_waste.is_some_and(nonnegative)
        || progress.return_ink_kg.is_some_and(|value| !nonnegative(value))
        || (progress.return_ink_kg.is_none() && !progress.returned_paint_report_attached)
        || [progress.produced_qty, progress.gross_qty, progress.finished_goods_kg,
            progress.finished_goods_meter, progress.bobina_kg, progress.diameter,
            progress.lamination_print_leftover_rolls, progress.lamination_film_leftover_rolls,
            progress.rezka_bosma_waste, progress.rezka_lamination_waste, progress.rezka_edge_waste]
            .iter().any(Option::is_some)
        || !progress.rezka_frames.is_empty() || progress.rezka_record_frame_index.is_some()
        || !progress.qr_payload.trim().is_empty()
        || progress.worker_handoff || progress.remove_roll_from_apparatus
        || progress.freeze_with_issue || !progress.freeze_request_id.is_empty()
    {
        return Err(ProductionMapError::ProgressInputInvalid);
    }
    Ok(())
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

include!("service_progress_support_parts/batch_status_for_progress_action.rs");

include!("service_progress_support_parts/progress_batch_payload.rs");
