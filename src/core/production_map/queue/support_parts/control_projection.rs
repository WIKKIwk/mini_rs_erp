pub(super) struct QueueControlOrderInput<'a> {
    pub(super) order_id: &'a str,
    pub(super) order_map: &'a ProductionMapDefinition,
    pub(super) batches: &'a [OrderProgressBatch],
    pub(super) opening_wip_records: &'a [OpeningWipRecord],
    pub(super) active_session: Option<&'a OrderRunSession>,
    pub(super) waiting_reentry_stage_node_id: Option<String>,
    pub(super) opening_wip_stage_node_id: Option<String>,
    waiting_opening_wip_stage_node_ids: BTreeSet<String>,
}

impl QueueControlOrderInput<'_> {
    pub(super) fn has_waiting_opening_wip_for_stage(&self, stage_node_id: &str) -> bool {
        self.waiting_opening_wip_stage_node_ids
            .contains(stage_node_id.trim())
    }
}

pub(super) fn queue_control_order_input<'a>(
    order_id: &'a str,
    order_map: &'a ProductionMapDefinition,
    apparatus: &str,
    batches: &'a [OrderProgressBatch],
    opening_wip_records: &'a [OpeningWipRecord],
    active_session: Option<&'a OrderRunSession>,
) -> QueueControlOrderInput<'a> {
    let mut opening_wip_stage_node_id = None;
    let mut waiting_opening_wip_stage_node_ids = BTreeSet::new();
    for record in opening_wip_records {
        if record.intake.status != OpeningWipIntakeStatus::Confirmed
            || !record
                .batches
                .iter()
                .any(|batch| batch.wip_status == OpeningWipBatchStatus::Waiting)
        {
            continue;
        }
        let target_stage_node_ids =
            opening_wip_target_stage_node_ids(order_map, &record.intake, apparatus);
        if opening_wip_stage_node_id.is_none() {
            opening_wip_stage_node_id = target_stage_node_ids.first().cloned();
        }
        waiting_opening_wip_stage_node_ids.extend(target_stage_node_ids);
    }

    QueueControlOrderInput {
        order_id,
        order_map,
        batches,
        opening_wip_records,
        active_session,
        waiting_reentry_stage_node_id: waiting_reentry_stage_node_id(
            order_map, batches, order_id, apparatus,
        ),
        opening_wip_stage_node_id,
        waiting_opening_wip_stage_node_ids,
    }
}

fn opening_wip_target_stage_node_ids(
    map: &ProductionMapDefinition,
    intake: &OpeningWipIntake,
    target_apparatus: &str,
) -> Vec<String> {
    if !intake.source_apparatus.trim().is_empty() {
        let Some(source_stage) = chain::work_stage_for_station(
            map,
            &intake.source_apparatus,
            &intake.resume_stage_node_id,
        ) else {
            return Vec::new();
        };
        return chain::next_work_stages_for_node(map, &source_stage.node_id)
            .into_iter()
            .filter(|stage| {
                stage.apparatus_id.as_deref().is_some_and(|apparatus_id| {
                    super::types::apparatus_ids_match(apparatus_id, target_apparatus)
                })
            })
            .map(|stage| stage.node_id.trim().to_string())
            .collect();
    }
    if !super::types::apparatus_ids_match(&intake.resume_apparatus, target_apparatus) {
        return Vec::new();
    }
    chain::work_stage_for_station(map, target_apparatus, &intake.resume_stage_node_id)
        .map(|stage| vec![stage.node_id.trim().to_string()])
        .unwrap_or_default()
}

pub(super) struct QueueControlStageProjection {
    pub(super) has_waiting_previous_stage_wip: bool,
    pub(super) input_contained_kadr_count: Option<usize>,
}

pub(super) fn queue_control_stage_projection(
    map: &ProductionMapDefinition,
    batches: &[OrderProgressBatch],
    order_id: &str,
    previous_stage: Option<&str>,
    apparatus: &str,
    stage_node_id: &str,
) -> QueueControlStageProjection {
    let previous_stage = previous_stage.map(str::trim).filter(|value| !value.is_empty());
    let mut has_waiting_previous_stage_wip = false;
    let mut input_contained_kadr_count = None;
    for batch in batches {
        let next_stage_node_id = progress_batch_next_stage_node_id(batch);
        if input_contained_kadr_count.is_none() && next_stage_node_id == stage_node_id {
            input_contained_kadr_count = batch
                .payload_json
                .get("contained_kadr_count")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0);
        }
        if has_waiting_previous_stage_wip {
            continue;
        }
        let Some(previous_stage) = previous_stage else {
            continue;
        };
        if batch.order_id.trim() == order_id.trim()
            && super::types::apparatus_ids_match(&batch.apparatus, previous_stage)
            && batch.action.records_progress_output()
            && (batch.next_apparatus.trim().is_empty()
                || chain::stage_ids_match_for_map(map, &batch.next_apparatus, apparatus))
            && (next_stage_node_id.is_empty()
                || chain::stage_node_ids_match_for_map(map, next_stage_node_id, stage_node_id))
            && batch.wip_status == OrderProgressBatchWipStatus::Waiting
        {
            has_waiting_previous_stage_wip = true;
        }
    }
    QueueControlStageProjection {
        has_waiting_previous_stage_wip,
        input_contained_kadr_count,
    }
}
