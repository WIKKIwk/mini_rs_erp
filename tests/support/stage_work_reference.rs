//! Frozen evaluator from f4286c7620a0372b64a23c64f293091bdd3cf9b7.
//! Only visibility and the input/event type imports differ from production.
use mini_rs_erp::core::production_map::verification_stage_work::{
    Event as ProductionStageLifecycleEvent, Input as StageWorkInput,
};
use mini_rs_erp::core::production_map::*;
use std::collections::{BTreeMap, BTreeSet};
const WORK_PROTOCOL: &str = "stage_work_protocol";
const WORK_REPORT: &str = "stage_work_report";
fn work_report(session: &OrderRunSession) -> Option<StageWorkReport> {
    serde_json::from_value(session.payload_json.get(WORK_REPORT)?.clone()).ok()
}
pub fn stage_work_statuses(
    map: &ProductionMapDefinition,
    sessions: &[OrderRunSession],
    inputs: &[StageWorkInput],
    states: &BTreeMap<String, BTreeMap<String, String>>,
    events: &[ProductionStageLifecycleEvent],
) -> Vec<StageWorkStatus> {
    let stages = chain::linear_work_stages(map)
        .into_iter()
        .filter(|s| s.apparatus_id.is_some())
        .collect::<Vec<_>>();
    let mut operations = BTreeMap::<String, Vec<&chain::ChainStage>>::new();
    let mut key_by_node = BTreeMap::new();
    for stage in &stages {
        let Some(node) = map.nodes.iter().find(|n| n.id == stage.node_id) else {
            continue;
        };
        let key = if node.alternative_group_id.trim().is_empty() {
            format!("node:{}", node.id)
        } else {
            format!("group:{}", node.alternative_group_id.trim())
        };
        key_by_node.insert(stage.node_id.as_str(), key.clone());
        operations.entry(key).or_default().push(stage);
    }
    let mut local_done = BTreeMap::new();
    let mut statuses = BTreeMap::new();
    let mut predecessors = BTreeMap::<String, BTreeSet<String>>::new();
    for (key, candidates) in &operations {
        let matches = |node: &str| candidates.iter().any(|s| s.node_id == node);
        let mut latest = BTreeMap::<&str, &OrderRunSession>::new();
        for session in sessions.iter().filter(|s| s.order_id == map.id) {
            // Missing occurrence metadata is not evidence for either of two
            // repeated uses of the same physical machine.
            if session.stage_node_id.trim().is_empty()
                && stages
                    .iter()
                    .filter(|s| s.apparatus_id.as_deref() == Some(session.apparatus.as_str()))
                    .count()
                    != 1
            {
                continue;
            }
            let node =
                chain::work_stage_for_station(map, &session.apparatus, &session.stage_node_id);
            if node.as_ref().is_none_or(|s| !matches(&s.node_id)) {
                continue;
            }
            let old = latest.entry(&session.apparatus).or_insert(session);
            if (session.started_at_unix, &session.session_id)
                > (old.started_at_unix, &old.session_id)
            {
                *old = session;
            }
        }
        let mut required = Vec::new();
        let mut last: Option<(&OrderRunSession, StageWorkReport)> = None;
        let done = if latest.is_empty() {
            let mut node_events = BTreeMap::new();
            for event in events.iter().filter(|e| matches(&e.stage_node_id)) {
                node_events.insert(&event.stage_node_id, event);
            }
            if !node_events.is_empty() {
                node_events.values().all(|e| {
                    e.action == queue_state::ApparatusQueueAction::Complete
                        && e.to_state == queue_state::ApparatusQueueOrderState::Completed
                })
            } else if candidates.len() == 1
                && stages
                    .iter()
                    .filter(|s| s.apparatus_id == candidates[0].apparatus_id)
                    .count()
                    == 1
            {
                states
                    .get(candidates[0].apparatus_id.as_deref().unwrap_or_default())
                    .and_then(|o| o.get(&map.id))
                    .is_some_and(|s| s == "completed")
            } else {
                false
            }
        } else {
            latest
                .values()
                .all(|s| s.status == OrderRunStatus::Completed)
        };
        for session in latest.values() {
            if let Some(report) = work_report(session) {
                if last
                    .as_ref()
                    .is_none_or(|(_, old)| report.sequence > old.sequence)
                {
                    last = Some((session, report));
                }
            } else if session.status == OrderRunStatus::Completed
                && session.payload_json.get(WORK_PROTOCOL).is_some()
            {
                required.push(session.apparatus.clone());
            }
        }
        let mut prev = BTreeSet::new();
        for candidate in candidates {
            for p in chain::previous_work_stages_for_node(map, &candidate.node_id) {
                if let Some(k) = key_by_node.get(p.node_id.as_str()).filter(|k| *k != key) {
                    prev.insert(k.clone());
                }
            }
        }
        let inputs_here = inputs
            .iter()
            .filter(|i| i.outstanding)
            .filter(|input| {
                if !input.target_node.is_empty() {
                    return matches(&input.target_node);
                }
                if !input.source_node.is_empty() {
                    return chain::next_work_stages_for_node(map, &input.source_node)
                        .iter()
                        .any(|s| matches(&s.node_id));
                }
                !input.target_apparatus.is_empty()
                    && candidates
                        .iter()
                        .any(|s| s.apparatus_id.as_deref() == Some(input.target_apparatus.as_str()))
                    || (!input.source_apparatus.is_empty()
                        && candidates.iter().any(|s| {
                            chain::previous_work_stage_stations(
                                map,
                                s.apparatus_id.as_deref().unwrap_or_default(),
                            )
                            .contains(&input.source_apparatus)
                        }))
            })
            .collect::<Vec<_>>();
        local_done.insert(
            key.clone(),
            done && required.is_empty() && inputs_here.is_empty(),
        );
        // Manual accounting stays available; final prompts wait until no
        // unclaimed input rolls remain for this operation.
        if inputs_here.iter().any(|i| i.available) {
            required.clear();
        }
        predecessors.insert(key.clone(), prev);
        statuses.insert(
            key.clone(),
            StageWorkStatus {
                stage_node_id: candidates[0].node_id.clone(),
                has_available_input: inputs_here.iter().any(|i| i.available),
                last_apparatus: last
                    .as_ref()
                    .map(|(s, _)| s.apparatus.clone())
                    .unwrap_or_default(),
                last_worker_ref: last
                    .as_ref()
                    .map(|(_, r)| r.worker_ref.clone())
                    .unwrap_or_default(),
                last_report_at_unix: last
                    .as_ref()
                    .map(|(_, r)| r.submitted_at_unix)
                    .unwrap_or_default(),
                astatka_required_apparatuses: required,
                ..Default::default()
            },
        );
    }
    // Monotone fixed point: a cycle or unresolved predecessor stays open.
    let mut closed = BTreeSet::new();
    for _ in 0..operations.len() {
        for key in operations.keys() {
            if local_done[key] && predecessors[key].iter().all(|p| closed.contains(p)) {
                closed.insert(key.clone());
            }
        }
    }
    for (key, status) in &mut statuses {
        status.upstream_closed = predecessors[key].iter().all(|p| closed.contains(p));
        status.completed = closed.contains(key);
        if !status.upstream_closed {
            status.astatka_required_apparatuses.clear();
        }
    }
    statuses.into_values().collect()
}

pub fn work_lifecycle(
    fallback: ProductionOrderLifecycleStatus,
    statuses: &[StageWorkStatus],
    sessions: &[OrderRunSession],
) -> ProductionOrderLifecycleStatus {
    if !sessions
        .iter()
        .any(|s| s.payload_json.get(WORK_PROTOCOL).is_some())
    {
        return fallback;
    }
    if !statuses.is_empty() && statuses.iter().all(|s| s.completed) {
        ProductionOrderLifecycleStatus::ProductionCompleted
    } else {
        ProductionOrderLifecycleStatus::InProgress
    }
}
