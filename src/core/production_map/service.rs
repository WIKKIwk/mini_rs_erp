#[path = "service_admin_pages.rs"]
mod admin_pages;
#[path = "service_display_order.rs"]
mod display_order;
#[path = "service_projection.rs"]
mod projection;

use super::*;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

use super::apparatus_resolver::CanonicalApparatusResolver;
pub use super::prepared_queue_action::PreparedApparatusQueueAction;
pub(super) use super::prepared_queue_action::QueueProgressRecords;
use super::progress::effective_apparatus_queue_policy_record;
use super::service_maps::compile_saved_maps_reusing_programs;
use super::store_port::ApparatusQueueStateMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OwnedRwLockWriteGuard, RwLock, broadcast};

#[path = "service_mutation_guard.rs"]
mod mutation_guard;
use mutation_guard::{ProductionMutationLocks, ScopedProductionMutationGuard};

const LIVE_NOTIFY_CAPACITY: usize = 256;

pub(crate) enum QueueProgressMutationGuard {
    Exclusive {
        _guard: OwnedRwLockWriteGuard<()>,
    },
    Scoped {
        _guard: ScopedProductionMutationGuard,
    },
}

#[path = "service_worker_snapshot.rs"]
mod worker_snapshot;
use worker_snapshot::WorkerSnapshotScope;

struct ProductionSnapshotCache {
    epoch: String,
    revision: AtomicU64,
    snapshot: RwLock<Option<CachedProductionSnapshot>>,
    rebuild_lock: Mutex<()>,
    worker_snapshots: Mutex<
        BTreeMap<WorkerSnapshotScope, std::sync::Arc<Mutex<Option<CachedProductionSnapshot>>>>,
    >,
    committed_events: std::sync::Mutex<CommittedLiveEvents>,
}

const COMMITTED_LIVE_EVENTS_LIMIT: usize = 2048;

#[derive(Default)]
struct CommittedLiveEvents {
    seen: BTreeSet<String>,
    order: std::collections::VecDeque<String>,
}

impl CommittedLiveEvents {
    fn remember(&mut self, event_id: &str) -> bool {
        if !self.seen.insert(event_id.to_string()) {
            return false;
        }
        self.order.push_back(event_id.to_string());
        while self.order.len() > COMMITTED_LIVE_EVENTS_LIMIT {
            if let Some(expired) = self.order.pop_front() {
                self.seen.remove(&expired);
            }
        }
        true
    }
}

impl Default for ProductionSnapshotCache {
    fn default() -> Self {
        Self {
            epoch: format!("{:032x}", rand::random::<u128>()),
            revision: AtomicU64::new(0),
            snapshot: RwLock::new(None),
            rebuild_lock: Mutex::new(()),
            worker_snapshots: Mutex::new(BTreeMap::new()),
            committed_events: std::sync::Mutex::new(CommittedLiveEvents::default()),
        }
    }
}

struct CachedProductionSnapshot {
    revision: u64,
    snapshot: std::sync::Arc<ProductionMapLiveSnapshot>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProductionMapLiveSnapshot {
    pub sequence_versions: BTreeMap<String, String>,
    pub sequence_revisions: BTreeMap<String, i64>,
    pub maps: Vec<ProductionMapSaved>,
    pub sequences: BTreeMap<String, Vec<String>>,
    pub visible_order_ids: BTreeMap<String, Vec<String>>,
    /// One-based worker display positions before queue search/windowing,
    /// scoped by apparatus and restricted to returned window rows.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub sequence_positions_by_apparatus: BTreeMap<String, BTreeMap<String, usize>>,
    pub queue_states: BTreeMap<String, BTreeMap<String, String>>,
    pub stage_states: BTreeMap<String, BTreeMap<String, String>>,
    pub queue_policies: Vec<ApparatusQueuePolicyRecord>,
    pub queue_action_controls: BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
    pub order_statuses: BTreeMap<String, ProductionOrderStatusDetail>,
    pub order_controls: BTreeMap<String, OrderControlRecord>,
    pub stage_order_controls: Vec<OrderControlRecord>,
    pub frozen_orders_by_apparatus: BTreeMap<String, Vec<FrozenOrderSnapshot>>,
}

fn stage_states_for_snapshot(
    projections: &[chain::ProjectionMap<'_>],
    output_order_ids: Option<&BTreeSet<String>>,
    controls: &BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
    logs_by_order: &BTreeMap<String, Vec<ProductionOrderLogEntry>>,
    stored_queue_states: &ApparatusQueueStateMap,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    // queue_action_controls_for_snapshot already keys controls by canonical apparatus ID.
    for projection in projections {
        let map = projection.map();
        if output_order_ids.is_some_and(|ids| !ids.contains(&map.id)) {
            continue;
        }
        let order_id = map.id.trim();
        if order_id.is_empty() {
            continue;
        }
        let stages = projection.stages()
            .into_iter()
            .filter(|stage| stage.apparatus_id.is_some())
            .collect::<Vec<_>>();
        if stages.is_empty() {
            continue;
        }
        let valid_stage_node_ids = stages
            .iter()
            .map(|stage| stage.node_id.trim())
            .collect::<BTreeSet<_>>();
        let mut states = BTreeMap::<String, String>::new();
        for log in logs_by_order.get(order_id).into_iter().flatten() {
            let stage_node_id = log.stage_node_id.trim();
            if valid_stage_node_ids.contains(stage_node_id) {
                states.insert(stage_node_id.to_string(), log.to_state.as_str().to_string());
            }
        }

        let mut stages_by_apparatus = BTreeMap::new();
        for stage in &stages {
            let Some(apparatus) = stage
                .apparatus_id
                .as_deref()
                .map(str::trim)
                .filter(|apparatus| !apparatus.is_empty())
            else {
                continue;
            };
            stages_by_apparatus
                .entry(apparatus)
                .or_insert_with(Vec::new)
                .push(stage);
        }
        for (apparatus, occurrences) in stages_by_apparatus {
            let control = controls
                .get(apparatus)
                .and_then(|orders| orders.get(order_id));
            let Some(control) = control else {
                continue;
            };
            // Global freeze blocks every apparatus's controls without claiming
            // that untouched downstream stages have started production.
            let stage_state = if control.state == queue_state::ApparatusQueueOrderState::Frozen {
                stored_queue_states
                    .get(apparatus)
                    .and_then(|orders| orders.get(order_id))
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    .unwrap_or(queue_state::ApparatusQueueOrderState::Pending)
                    .as_str()
            } else {
                control.state.as_str()
            };
            if occurrences.len() == 1 {
                states.insert(
                    occurrences[0].node_id.trim().to_string(),
                    stage_state.to_string(),
                );
                continue;
            }
            let current_index = occurrences
                .iter()
                .position(|stage| stage.node_id.trim() == control.stage_node_id.trim());
            let Some(current_index) = current_index else {
                continue;
            };
            for (index, stage) in occurrences.iter().enumerate() {
                let stage_node_id = stage.node_id.trim().to_string();
                if index < current_index {
                    states
                        .entry(stage_node_id)
                        .or_insert_with(|| "completed".to_string());
                } else if index == current_index {
                    states.insert(stage_node_id, stage_state.to_string());
                } else {
                    states
                        .entry(stage_node_id)
                        .or_insert_with(|| "pending".to_string());
                }
            }
        }
        for stage in stages {
            if let Some(work) = controls
                .get(stage.apparatus_id.as_deref().unwrap_or_default())
                .and_then(|orders| orders.get(order_id))
                .filter(|c| {
                    chain::stage_node_ids_match_for_map(map, &stage.node_id, &c.stage_node_id)
                })
                .and_then(|c| c.stage_work.as_ref())
                && work.completed
            {
                states.insert(stage.node_id.trim().to_string(), "completed".into());
            }
            states
                .entry(stage.node_id.trim().to_string())
                .or_insert_with(|| "pending".to_string());
        }
        result.insert(order_id.to_string(), states);
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProductionMapLiveEvent {
    Invalidate,
    PrintPreflight {
        revision: u64,
        order_id: String,
        apparatus: String,
    },
    Delta(std::sync::Arc<ProductionMapLiveDelta>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionMapLiveDelta {
    pub epoch: String,
    pub apparatus: String,
    pub base_revision: i64,
    pub revision: i64,
    pub ops: Vec<serde_json::Value>,
    pub version: String,
}

#[derive(Clone)]
pub struct ProductionMapService {
    pub(super) store: std::sync::Arc<dyn ProductionMapStorePort>,
    pub(super) apparatus_resolver: std::sync::Arc<dyn CanonicalApparatusResolver>,
    live_notify: broadcast::Sender<ProductionMapLiveEvent>,
    queue_action_locks: std::sync::Arc<ProductionMutationLocks>,
    snapshot_cache: std::sync::Arc<ProductionSnapshotCache>,
    #[cfg(any(test, feature = "projection-research"))]
    reference_projection: bool,
}

include!("service_parts/resolve_canonical_apparatus.rs");

include!("service_parts/build_production_snapshot_in_scope.rs");

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
