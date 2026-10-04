//! Narrow production-library adapter for the deep stage-closure parity suite.
//! Absent from ordinary builds; no alternate production evaluator or writes.
use std::collections::BTreeMap;

use super::*;

#[derive(Debug, Clone, Default)]
pub struct Input {
    pub source_node: String,
    pub target_node: String,
    pub source_apparatus: String,
    pub target_apparatus: String,
    pub outstanding: bool,
    pub available: bool,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub stage_node_id: String,
    pub action: queue_state::ApparatusQueueAction,
    pub to_state: queue_state::ApparatusQueueOrderState,
}

pub fn statuses(
    map: &ProductionMapDefinition,
    sessions: &[OrderRunSession],
    inputs: &[Input],
    states: &BTreeMap<String, BTreeMap<String, String>>,
    events: &[Event],
) -> Vec<StageWorkStatus> {
    let inputs = inputs
        .iter()
        .map(|i| stage_execution::StageWorkInput {
            source_node: i.source_node.clone(),
            target_node: i.target_node.clone(),
            source_apparatus: i.source_apparatus.clone(),
            target_apparatus: i.target_apparatus.clone(),
            outstanding: i.outstanding,
            available: i.available,
        })
        .collect::<Vec<_>>();
    let events = events
        .iter()
        .map(|e| ProductionStageLifecycleEvent {
            stage_node_id: e.stage_node_id.clone(),
            action: e.action,
            to_state: e.to_state,
        })
        .collect::<Vec<_>>();
    stage_execution::stage_work_statuses(map, sessions, &inputs, states, &events)
}

pub fn lifecycle(
    fallback: ProductionOrderLifecycleStatus,
    statuses: &[StageWorkStatus],
    sessions: &[OrderRunSession],
) -> ProductionOrderLifecycleStatus {
    stage_execution::work_lifecycle(fallback, statuses, sessions)
}

thread_local! {
    static RESOLUTIONS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn count_resolution() {
    RESOLUTIONS.with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + 1));
        }
    });
}

/// Count real resolver calls on this test thread. Disabled during timing runs.
pub fn measure_resolutions<T>(run: impl FnOnce() -> T) -> (T, usize) {
    RESOLUTIONS.with(|count| count.set(Some(0)));
    let result = run();
    let count = RESOLUTIONS.with(|count| count.replace(None).unwrap_or_default());
    (result, count)
}

/// Exercise the same scoped path used by print-preflight validation.
pub async fn scoped_queue_controls(
    service: &ProductionMapService,
    apparatus: &str,
) -> Result<serde_json::Value, ProductionMapError> {
    let controls = service
        .queue_action_controls_for_apparatus(apparatus)
        .await?;
    serde_json::to_value(controls).map_err(|_| ProductionMapError::StoreFailed)
}
