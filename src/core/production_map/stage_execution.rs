//! Operation closure is distinct from a machine finishing its own roll.
//! Candidates share an operation; only actual executions participate in closure.
use std::collections::{BTreeMap, BTreeSet};

use super::*;

pub(crate) const WORK_PROTOCOL: &str = "stage_work_protocol";
pub(crate) const WORK_REPORT: &str = "stage_work_report";

pub(crate) fn report_now() -> i64 {
    super::progress::unix_seconds()
}

#[derive(Debug, Clone)]
pub enum StageAstatkaReport {
    Bosma(BosmaAstatkaReport),
    Laminate(LaminatsiyaAstatkaReport),
    Cut(RezkaAstatkaReport),
}

impl StageAstatkaReport {
    pub fn identity(&self) -> (&str, &str, &str) {
        match self {
            Self::Bosma(r) => (&r.order_id, &r.apparatus, &r.report_id),
            Self::Laminate(r) => (&r.order_id, &r.apparatus, &r.report_id),
            Self::Cut(r) => (&r.order_id, &r.apparatus, &r.report_id),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StageWorkReport {
    pub report_id: String,
    pub sequence: u64,
    pub submitted_at_unix: i64,
    pub worker_ref: String,
    #[serde(default)]
    pub worker_role: String,
    pub worker_display_name: String,
}

pub(crate) fn work_report(session: &OrderRunSession) -> Option<StageWorkReport> {
    serde_json::from_value(session.payload_json.get(WORK_REPORT)?.clone()).ok()
}

pub(crate) fn stamp_work_report(
    session: &mut OrderRunSession,
    report_id: &str,
    sequence: u64,
    actor: &QueueActionActor,
    now: i64,
) {
    // A retry/report edit must not change the winner of an already reported execution.
    if session.status != OrderRunStatus::Completed || work_report(session).is_some() {
        return;
    }
    session.payload_json[WORK_PROTOCOL] = serde_json::json!(1);
    session.payload_json[WORK_REPORT] = serde_json::json!(StageWorkReport {
        report_id: report_id.into(),
        sequence,
        submitted_at_unix: now,
        worker_ref: actor.ref_.clone(),
        worker_role: actor.role.clone(),
        worker_display_name: actor.display_name.clone(),
    });
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StageWorkStatus {
    pub stage_node_id: String,
    pub upstream_closed: bool,
    pub completed: bool,
    pub last_apparatus: String,
    pub last_worker_ref: String,
    pub last_report_at_unix: i64,
    pub astatka_required_apparatuses: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct StageWorkControl {
    pub completed: bool,
    pub upstream_closed: bool,
    pub astatka_available: bool,
    pub astatka_required: bool,
    pub report_session_id: String,
    pub upstream_title: String,
    pub last_apparatus: String,
    pub last_worker_ref: String,
}

pub(crate) fn work_control(
    map: &ProductionMapDefinition,
    apparatus: &str,
    node: &str,
    statuses: &[StageWorkStatus],
    sessions: &[OrderRunSession],
) -> Option<StageWorkControl> {
    if !sessions
        .iter()
        .any(|s| s.payload_json.get(WORK_PROTOCOL).is_some())
    {
        return None;
    }
    let status = statuses
        .iter()
        .find(|s| chain::stage_node_ids_match_for_map(map, &s.stage_node_id, node))?;
    let latest = sessions
        .iter()
        .filter(|s| {
            s.apparatus == apparatus
                && chain::stage_node_ids_match_for_map(map, &s.stage_node_id, node)
        })
        .max_by(|a, b| (a.started_at_unix, &a.session_id).cmp(&(b.started_at_unix, &b.session_id)));
    Some(StageWorkControl {
        completed: status.completed,
        upstream_closed: status.upstream_closed,
        astatka_available: latest.is_some_and(|s| s.status == OrderRunStatus::Completed),
        astatka_required: status
            .astatka_required_apparatuses
            .iter()
            .any(|a| a == apparatus),
        report_session_id: latest.map(|s| s.session_id.clone()).unwrap_or_default(),
        upstream_title: chain::previous_work_stages_for_node(map, node)
            .iter()
            .map(|s| s.station_title.clone())
            .collect::<Vec<_>>()
            .join(", "),
        last_apparatus: status.last_apparatus.clone(),
        last_worker_ref: status.last_worker_ref.clone(),
    })
}

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
            target_node: b
                .payload_json
                .get("next_stage_node_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .into(),
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

pub(crate) fn stage_work_statuses(
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

pub(crate) fn work_lifecycle(
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

impl ProductionMapService {
    pub(crate) async fn astatka_execution_anchor(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        Ok(self
            .store
            .order_run_sessions_for_order(order_id)
            .await?
            .into_iter()
            .filter(|s| s.apparatus == apparatus)
            .max_by(|a, b| {
                (a.started_at_unix, &a.session_id).cmp(&(b.started_at_unix, &b.session_id))
            }))
    }
}

#[cfg(test)]
#[path = "stage_execution_tests.rs"]
mod tests;
