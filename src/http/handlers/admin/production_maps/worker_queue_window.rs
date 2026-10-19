use super::*;
use crate::core::production_map::{ApparatusQueuePolicy, ProductionMapLiveSnapshot};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

/// A read-only window over authoritative queues. Never use this projection to
/// validate a mutation: hidden orders still participate in all production rules.
#[derive(Clone, Default)]
pub(crate) struct WorkerQueueWindow {
    /// None preserves the legacy view; Some(0) explicitly selects all orders.
    pub limit: Option<usize>,
    search: String,
}

impl WorkerQueueWindow {
    pub fn new(limit: Option<usize>, search: &str, worker_scope: bool) -> Result<Self, AdminError> {
        let search = search.trim().to_lowercase();
        if limit.is_some_and(|n| ![0, 5, 10, 20, 50].contains(&n))
            || search.chars().count() > 120
            || (limit.is_none() && !search.is_empty())
            || (!worker_scope && (limit.is_some() || !search.is_empty()))
        {
            return Err(bad_request("invalid worker queue window"));
        }
        Ok(Self { limit, search })
    }

    pub fn history_limit(&self) -> usize {
        match self.limit {
            Some(0) => usize::MAX,
            Some(limit) => limit,
            None => 200,
        }
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
        let limit = match self.limit.expect("window projection requires a limit") {
            0 => usize::MAX,
            limit => limit,
        };
        let maps = snapshot
            .maps
            .iter()
            .map(|saved| (saved.map.id.as_str(), saved))
            .collect::<BTreeMap<_, _>>();
        let mut selected = history.iter().cloned().collect::<BTreeSet<_>>();
        let mut windows = BTreeMap::new();
        for (apparatus, visible_order_ids) in &snapshot.visible_order_ids {
            if !apparatus_scope.contains(apparatus) {
                continue;
            }
            let free_pick = snapshot.queue_policies.iter().any(|policy| {
                policy.apparatus_id.as_str() == apparatus
                    && policy.policy == ApparatusQueuePolicy::FreePick
            });
            let sequence: Cow<'_, [String]> = if free_pick {
                // Read the shared display order directly, without rebuilding
                // the manual sequence or copying its order IDs.
                Cow::Borrowed(visible_order_ids.as_slice())
            } else {
                let mut sequence = snapshot
                    .sequences
                    .get(apparatus)
                    .cloned()
                    .unwrap_or_default();
                let mut seen = sequence.iter().cloned().collect::<BTreeSet<_>>();
                sequence.extend(
                    visible_order_ids
                        .iter()
                        .filter(|id| seen.insert((*id).clone()))
                        .cloned(),
                );
                Cow::Owned(sequence)
            };
            let visible = visible_order_ids
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            let mut ids = sequence
                .iter()
                .filter(|id| visible.contains(id.as_str()))
                .filter(|id| {
                    maps.get(id.as_str()).is_some_and(|saved| {
                        if snapshot.worker_queue_order_hidden(id, free_pick) {
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
