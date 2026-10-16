use super::*;
use crate::core::production_map::{ApparatusQueuePolicy, ProductionMapLiveSnapshot};
use std::collections::{BTreeMap, BTreeSet};

/// A read-only window over authoritative queues. Never use this projection to
/// validate a mutation: hidden orders still participate in all production rules.
#[derive(Clone, Default)]
pub(crate) struct WorkerQueueWindow {
    pub limit: Option<usize>,
    search: String,
}

impl WorkerQueueWindow {
    pub fn new(limit: Option<usize>, search: &str, worker_scope: bool) -> Result<Self, AdminError> {
        let search = search.trim().to_lowercase();
        if limit.is_some_and(|n| ![5, 10, 20, 50].contains(&n))
            || search.chars().count() > 120
            || (limit.is_none() && !search.is_empty())
            || (!worker_scope && (limit.is_some() || !search.is_empty()))
        {
            return Err(bad_request("invalid worker queue window"));
        }
        Ok(Self { limit, search })
    }

    pub fn history_limit(&self) -> usize {
        self.limit.unwrap_or(200)
    }

    pub fn search_active(&self) -> bool {
        !self.search.is_empty()
    }

    pub fn scope(&self, assignment_scope: &str) -> String {
        use sha2::{Digest, Sha256};
        let Some(limit) = self.limit else {
            return assignment_scope.to_string();
        };
        let mut digest = Sha256::new();
        digest.update(format!("{assignment_scope}\0{limit}\0{}", self.search));
        format!("{:x}", digest.finalize())[..16].to_string()
    }

    pub fn project(
        &self,
        snapshot: &ProductionMapLiveSnapshot,
        apparatus_scope: &[String],
        history: &[String],
        customers: &BTreeMap<String, String>,
    ) -> ProductionMapLiveSnapshot {
        let limit = self.limit.expect("window projection requires a limit");
        let maps = snapshot
            .maps
            .iter()
            .map(|saved| (saved.map.id.as_str(), saved))
            .collect::<BTreeMap<_, _>>();
        let mut selected = history.iter().cloned().collect::<BTreeSet<_>>();
        let mut windows = BTreeMap::new();
        for (apparatus, visible) in &snapshot.visible_order_ids {
            if !apparatus_scope.contains(apparatus) {
                continue;
            }
            let visible = visible.iter().map(String::as_str).collect::<BTreeSet<_>>();
            let mut sequence = snapshot
                .sequences
                .get(apparatus)
                .cloned()
                .unwrap_or_default();
            let mut seen = sequence.iter().cloned().collect::<BTreeSet<_>>();
            sequence.extend(
                snapshot.visible_order_ids[apparatus]
                    .iter()
                    .filter(|id| seen.insert((*id).clone()))
                    .cloned(),
            );
            let mut ids = sequence
                .iter()
                .filter(|id| visible.contains(id.as_str()))
                .filter(|id| {
                    maps.get(id.as_str()).is_some_and(|saved| {
                        let frozen =
                            snapshot
                                .order_controls
                                .get(id.as_str())
                                .is_some_and(|control| {
                                    control.state
                                        == crate::core::production_map::OrderControlState::Frozen
                                })
                                || snapshot.queue_states.values().any(|states| {
                                    states.get(id.as_str()).is_some_and(|s| s == "frozen")
                                });
                        if frozen
                            || snapshot
                                .order_statuses
                                .get(id.as_str())
                                .is_some_and(|status| status.order_status == "frozen")
                        {
                            return false;
                        }
                        if self.search.is_empty() {
                            return true;
                        }
                        let map = &saved.map;
                        let haystack = [
                            map.id.as_str(),
                            map.code.as_str(),
                            map.order_number.as_str(),
                            map.title.as_str(),
                            map.product_code.as_str(),
                            map.customer_name.as_str(),
                            customers.get(id.as_str()).map(String::as_str).unwrap_or(""),
                        ]
                        .into_iter()
                        .chain(map.nodes.iter().map(|node| node.title.as_str()))
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_lowercase();
                        haystack.contains(&self.search)
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            if snapshot.queue_policies.iter().any(|policy| {
                policy.apparatus_id.as_str() == apparatus
                    && policy.policy == ApparatusQueuePolicy::FreePick
            }) {
                let controls = snapshot.queue_action_controls.get(apparatus);
                // Stable sort preserves the administrator's order for ties.
                ids.sort_by_key(|id| {
                    let control = controls.and_then(|controls| controls.get(id));
                    let state = control
                        .map(|c| c.state.as_str())
                        .or_else(|| {
                            snapshot
                                .queue_states
                                .get(apparatus)
                                .and_then(|s| s.get(id))
                                .map(String::as_str)
                        })
                        .unwrap_or("pending");
                    let worked = control.map(|c| c.last_worked_at_unix).unwrap_or(0);
                    let priority = if matches!(state, "in_progress" | "print_preflight") {
                        0
                    } else if state == "paused" || worked > 0 {
                        1
                    } else {
                        2
                    };
                    (priority, std::cmp::Reverse(worked))
                });
            }
            // A strict queue keeps its stored order, but an already running
            // job must remain reachable even if its position is outside N.
            let active = |id: &String| {
                snapshot
                    .queue_action_controls
                    .get(apparatus)
                    .and_then(|controls| controls.get(id))
                    .is_some_and(|control| {
                        matches!(control.state.as_str(), "in_progress" | "print_preflight")
                    })
            };
            let mut included = ids.iter().take(limit).cloned().collect::<BTreeSet<_>>();
            for id in ids.iter().filter(|id| active(id)) {
                if !included.contains(id) {
                    if let Some(replace) = ids
                        .iter()
                        .rev()
                        .find(|candidate| included.contains(*candidate) && !active(candidate))
                    {
                        included.remove(replace);
                        included.insert(id.clone());
                    }
                }
            }
            ids.retain(|id| included.contains(id));
            selected.extend(ids.iter().cloned());
            windows.insert(apparatus.clone(), ids);
        }
        fn keep<T: Clone>(
            source: &BTreeMap<String, T>,
            selected: &BTreeSet<String>,
        ) -> BTreeMap<String, T> {
            source
                .iter()
                .filter(|(id, _)| selected.contains(*id))
                .map(|(id, value)| (id.clone(), value.clone()))
                .collect()
        }
        ProductionMapLiveSnapshot {
            maps: snapshot
                .maps
                .iter()
                .filter(|saved| selected.contains(&saved.map.id))
                .cloned()
                .collect(),
            sequences: windows.clone(),
            visible_order_ids: windows,
            sequence_versions: snapshot
                .sequence_versions
                .iter()
                .filter(|(id, _)| apparatus_scope.contains(id))
                .map(|(id, value)| (id.clone(), value.clone()))
                .collect(),
            sequence_revisions: snapshot
                .sequence_revisions
                .iter()
                .filter(|(id, _)| apparatus_scope.contains(id))
                .map(|(id, value)| (id.clone(), *value))
                .collect(),
            queue_states: snapshot
                .queue_states
                .iter()
                .map(|(id, values)| (id.clone(), keep(values, &selected)))
                .collect(),
            queue_action_controls: snapshot
                .queue_action_controls
                .iter()
                .map(|(id, values)| (id.clone(), keep(values, &selected)))
                .collect(),
            stage_states: keep(&snapshot.stage_states, &selected),
            order_statuses: keep(&snapshot.order_statuses, &selected),
            order_controls: keep(&snapshot.order_controls, &selected),
            queue_policies: snapshot.queue_policies.clone(),
            frozen_orders_by_apparatus: BTreeMap::new(),
        }
    }
}
