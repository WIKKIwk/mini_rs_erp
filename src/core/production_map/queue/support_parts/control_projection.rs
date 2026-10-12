pub(super) struct QueueControlOrderInput<'a> {
    pub(super) order_id: &'a str,
    pub(super) order_map: &'a ProductionMapDefinition,
    pub(super) projection: &'a chain::ProjectionMap<'a>,
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
    projection: &'a chain::ProjectionMap<'a>,
    apparatus: &str,
    batches: &'a [OrderProgressBatch],
    opening_wip_records: &'a [OpeningWipRecord],
    active_session: Option<&'a OrderRunSession>,
) -> QueueControlOrderInput<'a> {
    let order_map = projection.map();
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
            opening_wip_target_stage_node_ids(projection, &record.intake, apparatus);
        if opening_wip_stage_node_id.is_none() {
            opening_wip_stage_node_id = target_stage_node_ids.first().cloned();
        }
        waiting_opening_wip_stage_node_ids.extend(target_stage_node_ids);
    }

    QueueControlOrderInput {
        order_id,
        order_map,
        projection,
        batches,
        opening_wip_records,
        active_session,
        waiting_reentry_stage_node_id: waiting_reentry_stage_node_id(
            projection, batches, order_id, apparatus,
        ),
        opening_wip_stage_node_id,
        waiting_opening_wip_stage_node_ids,
    }
}

fn opening_wip_target_stage_node_ids(
    projection: &chain::ProjectionMap<'_>,
    intake: &OpeningWipIntake,
    target_apparatus: &str,
) -> Vec<String> {
    if !intake.source_apparatus.trim().is_empty() {
        let Some(source_stage) =
            projection.work_stage(&intake.source_apparatus, &intake.resume_stage_node_id)
        else {
            return Vec::new();
        };
        return projection
            .next_stages(&source_stage.node_id)
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
    projection
        .work_stage(target_apparatus, &intake.resume_stage_node_id)
        .map(|stage| vec![stage.node_id.trim().to_string()])
        .unwrap_or_default()
}

pub(super) struct QueueControlStageProjection {
    pub(super) has_waiting_previous_stage_wip: bool,
    pub(super) input_contained_kadr_count: Option<usize>,
}

pub(super) fn queue_control_stage_projection(
    projection: &chain::ProjectionMap<'_>,
    batches: &[OrderProgressBatch],
    order_id: &str,
    previous_stage: Option<&str>,
    apparatus: &str,
    stage_node_id: &str,
) -> QueueControlStageProjection {
    let map = projection.map();
    let previous_stage = previous_stage
        .map(str::trim)
        .filter(|value| !value.is_empty());
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
        let Some(_) = previous_stage else {
            continue;
        };
        if batch.order_id.trim() == order_id.trim()
            && super::wip_route::actionable_wip(batch)
            && projection.route(batch).is_ok_and(|route| {
                route
                    .consumer_apparatus_ids
                    .iter()
                    .any(|id| id == apparatus)
                    && chain::stage_node_ids_match_for_map(map, &route.stage_node_id, stage_node_id)
            })
        {
            has_waiting_previous_stage_wip = true;
        }
    }
    QueueControlStageProjection {
        has_waiting_previous_stage_wip,
        input_contained_kadr_count,
    }
}

// Preserve the first-map lookup semantics even for malformed duplicate IDs.
pub(super) fn projections_by_order_id<'a>(
    projections: &'a [chain::ProjectionMap<'a>],
) -> std::collections::HashMap<&'a str, &'a chain::ProjectionMap<'a>> {
    let mut result = std::collections::HashMap::new();
    for projection in projections {
        let id = projection.map().id.trim();
        if !id.is_empty() {
            result.entry(id).or_insert(projection);
        }
    }
    result
}
