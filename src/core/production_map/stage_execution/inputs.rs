use super::*;

/// Compact input location used by both PostgreSQL and in-memory projection.
#[derive(Debug, Clone, Default)]
pub(crate) struct StageWorkInput {
    pub source_node: String,
    pub target_node: String,
    pub source_apparatus: String,
    pub target_apparatus: String,
    pub outstanding: bool,
    pub available: bool,
}

pub(crate) fn work_inputs(
    map: &ProductionMapDefinition,
    batches: &[OrderProgressBatch],
    opening: &[OpeningWipRecord],
) -> Vec<StageWorkInput> {
    work_inputs_projected(&chain::ProjectionMap::reference(map), batches, opening)
}

pub(crate) fn work_inputs_projected(
    projection: &chain::ProjectionMap<'_>,
    batches: &[OrderProgressBatch],
    opening: &[OpeningWipRecord],
) -> Vec<StageWorkInput> {
    let mut inputs = batches
        .iter()
        .map(|b| StageWorkInput {
            source_node: b
                .payload_json
                .get("stage_node_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .into(),
            target_node: if projection.is_cached()
                && b.wip_status == OrderProgressBatchWipStatus::Processed
            {
                // Terminal inputs are filtered out by stage_work_statuses.
                super::service_queue_support::progress_batch_next_stage_node_id(b).to_string()
            } else {
                projection
                    .route(b)
                    .map(|route| route.stage_node_id)
                    .unwrap_or_else(|_| {
                        super::service_queue_support::progress_batch_next_stage_node_id(b)
                            .to_string()
                    })
            },
            source_apparatus: b.apparatus.clone(),
            target_apparatus: b.next_apparatus.clone(),
            available: b.wip_status == OrderProgressBatchWipStatus::Waiting,
            outstanding: matches!(
                b.wip_status,
                OrderProgressBatchWipStatus::Waiting | OrderProgressBatchWipStatus::InUse
            ),
        })
        .collect::<Vec<_>>();
    inputs.extend(
        opening
            .iter()
            .filter(|r| r.intake.status == OpeningWipIntakeStatus::Confirmed)
            .filter(|r| {
                r.batches.iter().any(|b| {
                    matches!(
                        b.wip_status,
                        OpeningWipBatchStatus::Waiting | OpeningWipBatchStatus::InUse
                    )
                })
            })
            .map(|r| StageWorkInput {
                source_node: if r.intake.source_apparatus.is_empty() {
                    String::new()
                } else {
                    r.intake.resume_stage_node_id.clone()
                },
                target_node: if r.intake.source_apparatus.is_empty() {
                    r.intake.resume_stage_node_id.clone()
                } else {
                    String::new()
                },
                source_apparatus: r.intake.source_apparatus.clone(),
                target_apparatus: r.intake.resume_apparatus.clone(),
                outstanding: true,
                available: r
                    .batches
                    .iter()
                    .any(|b| b.wip_status == OpeningWipBatchStatus::Waiting),
            }),
    );
    inputs
}
