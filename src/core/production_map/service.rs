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

impl ProductionMapService {
    /// Resolve live apparatus configuration by immutable canonical identity.
    /// Missing or invalid configuration returns `StoreFailed`; callers cannot
    /// continue with a title/name-derived fallback.
    pub(crate) async fn resolve_canonical_apparatus(
        &self,
        apparatus_id: &crate::core::apparatus_standard::ApparatusId,
    ) -> Result<
        std::sync::Arc<crate::core::apparatus_standard::RuntimeApparatusConfiguration>,
        ProductionMapError,
    > {
        let configuration = self
            .apparatus_resolver
            .resolve(apparatus_id)
            .await?
            .ok_or(ProductionMapError::StoreFailed)?;
        if configuration.runtime.apparatus_id != *apparatus_id
            || !configuration.has_coherent_source()
            || !configuration.is_active()
        {
            return Err(ProductionMapError::StoreFailed);
        }
        Ok(configuration)
    }

    pub(crate) async fn resolve_canonical_apparatus_text(
        &self,
        value: &str,
    ) -> Result<
        std::sync::Arc<crate::core::apparatus_standard::RuntimeApparatusConfiguration>,
        ProductionMapError,
    > {
        let id = crate::core::apparatus_standard::ApparatusId::new(value.trim().to_string())
            .map_err(|_| ProductionMapError::StoreFailed)?;
        self.resolve_canonical_apparatus(&id).await
    }

    pub(crate) async fn active_canonical_apparatuses(
        &self,
    ) -> Result<
        Vec<std::sync::Arc<crate::core::apparatus_standard::RuntimeApparatusConfiguration>>,
        ProductionMapError,
    > {
        let mut configurations = Vec::new();
        for configuration in self.apparatus_resolver.list().await? {
            if !configuration.has_coherent_source() {
                return Err(ProductionMapError::StoreFailed);
            }
            if !configuration.is_active() {
                continue;
            }
            configurations.push(configuration);
        }
        Ok(configurations)
    }

    /// Snapshot-only variant of [`Self::active_canonical_apparatuses`].
    ///
    /// The live stream must keep serving every healthy apparatus even while one
    /// configuration is mid-migration or incoherent. Write paths keep using the
    /// strict variant above and stay fail-closed.
    pub(crate) async fn snapshot_canonical_apparatuses(
        &self,
    ) -> Vec<std::sync::Arc<crate::core::apparatus_standard::RuntimeApparatusConfiguration>> {
        let configurations = match self.apparatus_resolver.list().await {
            Ok(configurations) => configurations,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "live snapshot continues without canonical apparatuses"
                );
                return Vec::new();
            }
        };
        configurations
            .into_iter()
            .filter(|configuration| {
                if !configuration.has_coherent_source() {
                    tracing::warn!(
                        apparatus = %configuration.runtime.apparatus_id.as_str(),
                        "skipping incoherent apparatus in live snapshot"
                    );
                    return false;
                }
                configuration.is_active()
            })
            .collect()
    }

    pub(crate) async fn queue_action_guard(&self) -> OwnedRwLockWriteGuard<()> {
        self.queue_action_locks.barrier.clone().write_owned().await
    }

    pub(crate) async fn apparatus_action_guard(
        &self,
        apparatus: &str,
    ) -> ScopedProductionMutationGuard {
        self.queue_action_locks
            .scoped(vec![("apparatus", apparatus.trim().to_string())])
            .await
    }

    pub(crate) async fn apparatus_preflight_guard(
        &self,
        apparatus: &str,
        idempotency_key: &str,
    ) -> ScopedProductionMutationGuard {
        self.queue_action_locks
            .scoped(vec![
                ("apparatus", apparatus.trim().to_string()),
                (
                    "print-preflight-idempotency",
                    idempotency_key.trim().to_string(),
                ),
            ])
            .await
    }

    pub(crate) async fn wip_action_guard(&self, batch_id: &str) -> ScopedProductionMutationGuard {
        self.queue_action_locks
            .scoped(vec![("wip", batch_id.trim().to_string())])
            .await
    }

    pub(crate) async fn queue_progress_action_guard(
        &self,
        apparatus: &str,
        action: queue_state::ApparatusQueueAction,
        progress: &QueueProgressInput,
        material_barcodes: &[String],
        qolip_codes: &[String],
    ) -> Result<QueueProgressMutationGuard, ProductionMapError> {
        if action == queue_state::ApparatusQueueAction::Freeze
            || progress.freeze_with_issue
            || !progress.freeze_request_id.trim().is_empty()
        {
            return Ok(QueueProgressMutationGuard::Exclusive {
                _guard: self.queue_action_guard().await,
            });
        }
        let mut keys = vec![("apparatus", apparatus.trim().to_string())];
        let batch_id = progress.progress_batch_id.trim();
        let qr_payload = progress.qr_payload.trim();
        if !batch_id.is_empty() {
            keys.push(("wip", batch_id.to_string()));
        } else if !qr_payload.is_empty() {
            // Resolve QR-only clients to the same identity as clients supplying
            // batch_id. This read never grants ownership: prepare and commit
            // still revalidate the input after acquiring the guard.
            if let Some(batch) = self.store.progress_batch_by_qr(qr_payload).await? {
                keys.push(("wip", batch.batch_id));
            } else if let Some(batch) = self.store.opening_wip_batch("", qr_payload).await? {
                keys.push(("wip", batch.batch.batch_id));
            } else {
                keys.push(("wip-qr", qr_payload.to_ascii_lowercase()));
            }
        }
        keys.extend(
            material_barcodes
                .iter()
                .filter(|code| !code.trim().is_empty())
                .map(|code| ("material", code.trim().to_ascii_lowercase())),
        );
        keys.extend(
            qolip_codes
                .iter()
                .filter(|code| !code.trim().is_empty())
                .map(|code| ("qolip", code.trim().to_ascii_lowercase())),
        );
        Ok(QueueProgressMutationGuard::Scoped {
            _guard: self.queue_action_locks.scoped(keys).await,
        })
    }

    pub fn subscribe_live(&self) -> broadcast::Receiver<ProductionMapLiveEvent> {
        self.live_notify.subscribe()
    }

    pub fn notify_live(&self) {
        self.snapshot_cache.revision.fetch_add(1, Ordering::AcqRel);
        let _ = self.live_notify.send(ProductionMapLiveEvent::Invalidate);
    }

    pub fn notify_live_delta(&self, delta: ProductionMapLiveDelta) {
        self.snapshot_cache.revision.fetch_add(1, Ordering::AcqRel);
        let _ = self
            .live_notify
            .send(ProductionMapLiveEvent::Delta(std::sync::Arc::new(delta)));
    }

    pub fn snapshot_epoch(&self) -> &str {
        &self.snapshot_cache.epoch
    }

    pub fn snapshot_revision(&self) -> u64 {
        self.snapshot_cache.revision.load(Ordering::Acquire)
    }

    pub(crate) fn notify_print_preflight(&self, order_id: &str, apparatus: &str) {
        let revision = self.snapshot_cache.revision.fetch_add(1, Ordering::AcqRel) + 1;
        let _ = self
            .live_notify
            .send(ProductionMapLiveEvent::PrintPreflight {
                revision,
                order_id: order_id.to_string(),
                apparatus: apparatus.to_string(),
            });
    }

    /// The ID is generated by persistence, included in the committed PG
    /// notification, and returned only after commit. Either publisher may win;
    /// claiming and sending are synchronous under one short mutex so a
    /// cancelled request cannot leave a claimed-but-unpublished commit.
    pub(crate) fn notify_print_preflight_once(
        &self,
        event_id: Option<&str>,
        order_id: &str,
        apparatus: &str,
    ) -> bool {
        let Some(event_id) = event_id else {
            self.notify_print_preflight(order_id, apparatus);
            return true;
        };
        self.notify_committed_event_once(event_id, || {
            self.notify_print_preflight(order_id, apparatus);
        })
    }

    /// A bounded exact-ID registry shared by local mutation publishers and the
    /// database listener. Eviction permits an extra invalidation, never loss.
    pub(crate) fn notify_committed_event_once(
        &self,
        event_id: &str,
        publish: impl FnOnce(),
    ) -> bool {
        self.publish_committed_events(&[Some(event_id)], |fresh| {
            if fresh.is_empty() {
                return false;
            }
            publish();
            true
        })
    }

    /// Deduplicate before deciding whether a coalesced batch is scoped.
    /// Already-published local commits must not turn an unrelated fresh
    /// preflight event into a generic invalidation.
    pub(crate) fn notify_print_preflight_batch(
        &self,
        changes: &[(Option<&str>, &str, &str)],
        generic: bool,
    ) {
        let ids = changes.iter().map(|change| change.0).collect::<Vec<_>>();
        self.publish_committed_events(&ids, |fresh| {
            let target = fresh.first().map(|index| changes[*index]);
            if generic
                || target.is_some_and(|target| {
                    fresh.iter().any(|index| {
                        let change = changes[*index];
                        change.1 != target.1 || change.2 != target.2
                    })
                })
            {
                self.notify_live();
            } else if let Some((_, order, apparatus)) = target {
                self.notify_print_preflight(order, apparatus);
            }
        });
    }

    fn publish_committed_events<T>(
        &self,
        ids: &[Option<&str>],
        publish: impl FnOnce(&[usize]) -> T,
    ) -> T {
        let mut events = self
            .snapshot_cache
            .committed_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let fresh = ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                if id.is_none_or(|id| events.remember(id)) {
                    Some(index)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        // Keep the guard through publication. No await or user I/O is allowed
        // in these internal callbacks.
        publish(&fresh)
    }

    pub async fn queue_events_replay(
        &self,
        apparatus: &str,
        from_rev: i64,
        to_rev: i64,
    ) -> Result<Vec<serde_json::Value>, ProductionMapError> {
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        self.store
            .queue_events_replay(canonical.runtime.apparatus_id.as_str(), from_rev, to_rev)
            .await
    }

    pub async fn live_snapshot(&self) -> Result<ProductionMapLiveSnapshot, ProductionMapError> {
        Ok(self.live_snapshot_shared().await?.as_ref().clone())
    }

    /// Shared snapshot together with the revision it was built from.
    ///
    /// The revision is a monotonic counter bumped by every [`Self::notify_live`]
    /// call. Live-stream payloads expose it as `rev` so clients can detect a
    /// missed update and resync instead of sitting on stale state.
    pub async fn live_snapshot_shared_with_revision(
        &self,
    ) -> Result<(std::sync::Arc<ProductionMapLiveSnapshot>, u64), ProductionMapError> {
        loop {
            let revision = self.snapshot_cache.revision.load(Ordering::Acquire);
            {
                let cached = self.snapshot_cache.snapshot.read().await;
                if let Some(entry) = cached.as_ref()
                    && entry.revision == revision
                {
                    return Ok((entry.snapshot.clone(), entry.revision));
                }
            }

            let _rebuild_guard = self.snapshot_cache.rebuild_lock.lock().await;
            let revision = self.snapshot_cache.revision.load(Ordering::Acquire);
            {
                let cached = self.snapshot_cache.snapshot.read().await;
                if let Some(entry) = cached.as_ref()
                    && entry.revision == revision
                {
                    return Ok((entry.snapshot.clone(), entry.revision));
                }
            }

            let snapshot = self.build_production_snapshot().await?;
            let latest_revision = self.snapshot_cache.revision.load(Ordering::Acquire);
            if latest_revision != revision {
                continue;
            }

            let snapshot = std::sync::Arc::new(snapshot);
            *self.snapshot_cache.snapshot.write().await = Some(CachedProductionSnapshot {
                revision,
                snapshot: snapshot.clone(),
            });
            return Ok((snapshot, revision));
        }
    }

    pub async fn live_snapshot_shared(
        &self,
    ) -> Result<std::sync::Arc<ProductionMapLiveSnapshot>, ProductionMapError> {
        Ok(self.live_snapshot_shared_with_revision().await?.0)
    }

    async fn build_production_snapshot(
        &self,
    ) -> Result<ProductionMapLiveSnapshot, ProductionMapError> {
        let previous = self
            .snapshot_cache
            .snapshot
            .read()
            .await
            .as_ref()
            .map(|entry| entry.snapshot.clone());
        self.build_production_snapshot_in_scope(None, previous.as_deref())
            .await
    }

    async fn build_production_snapshot_in_scope(
        &self,
        scope: Option<&WorkerSnapshotScope>,
        previous: Option<&ProductionMapLiveSnapshot>,
    ) -> Result<ProductionMapLiveSnapshot, ProductionMapError> {
        let (
            raw_maps,
            (stored_sequences, sequence_revisions),
            mut queue_states,
            order_controls,
            output_scope,
        ) = if let Some(scope) = scope {
            let inputs = self.worker_snapshot_inputs(scope).await?;
            (
                inputs.maps,
                inputs.sequences,
                inputs.queue_states,
                inputs.order_controls,
                Some((inputs.order_ids, inputs.apparatus_ids)),
            )
        } else {
            let (maps, sequences, states, controls) = tokio::try_join!(
                self.store.maps(),
                self.store.apparatus_sequences_with_revisions(),
                self.store.apparatus_queue_states(),
                self.store.order_control_states(),
            )?;
            (maps, sequences, states, controls, None)
        };
        let stage_order_controls = self.store.stage_order_control_states().await?;
        let canonical_apparatuses = self.snapshot_canonical_apparatuses().await;

        // One immutable topology cache serves every phase of this rebuild.
        // Queue visibility, sequence versions, controls and presentation must
        // not independently traverse the same dependency maps.
        let projections = raw_maps
            .iter()
            .map(|map| self.projection_map(map))
            .collect::<Vec<_>>();
        let mut projections_by_order = BTreeMap::new();
        for projection in &projections {
            projections_by_order.entry(projection.map().id.as_str()).or_insert(projection);
        }
        let unfiltered_visible = apparatus::visible_order_ids_by_projection(&projections);
        let mut visible_order_ids = unfiltered_visible.clone();
        apparatus::filter_unselected_print_orders(
            &raw_maps,
            &canonical_apparatuses,
            &mut visible_order_ids,
        );
        let frozen_order_ids = order_controls
            .iter()
            .filter_map(|(id, control)| {
                (control.state == OrderControlState::Frozen).then_some(id.clone())
            })
            .collect::<BTreeSet<_>>();
        let mut sequences = Self::effective_apparatus_sequences_for_visible(
            &unfiltered_visible,
            &stored_sequences,
            &frozen_order_ids,
        );
        let holds = self.store.active_print_preflight_holds().await?;
        let sequence_versions = {
            let mut sequence_versions = BTreeMap::new();
            for canonical in &canonical_apparatuses {
                let id = canonical.runtime.apparatus_id.as_str();
                if scope.is_some_and(|scope| {
                    if scope.only_orders {
                        !output_scope
                            .as_ref()
                            .is_some_and(|(_, apparatus)| apparatus.contains(id))
                    } else {
                        !scope.apparatus.iter().any(|assigned| assigned == id)
                    }
                }) {
                    continue;
                }
                let preflight = holds
                    .iter()
                    .filter(|h| h.apparatus == id && h.is_live_at(0))
                    .map(|h| h.order_id.clone())
                    .collect();
                let state = SequenceMoveState::from_projection(
                    canonical,
                    &projections,
                    stored_sequences
                        .get(id)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    queue_states.get(id).unwrap_or(&BTreeMap::new()),
                    &frozen_order_ids,
                    &preflight,
                );
                sequence_versions.insert(id.to_string(), state.version());
            }
            sequence_versions
        };
        apparatus::filter_unselected_print_orders(
            &raw_maps,
            &canonical_apparatuses,
            &mut sequences,
        );
        let queue_policies = canonical_apparatuses
            .iter()
            .map(|canonical| effective_apparatus_queue_policy_record(canonical))
            .collect::<Vec<_>>();
        let order_ids = raw_maps
            .iter()
            .filter(|map| {
                output_scope
                    .as_ref()
                    .is_none_or(|(ids, _)| ids.contains(&map.id))
            })
            .map(|map| map.id.trim().to_string())
            .filter(|order_id| !order_id.is_empty())
            .collect::<Vec<_>>();
        let (queue_action_controls, queue_logs_by_order, lifecycles) = tokio::try_join!(
            async {
                if let Some((ids, apparatus_ids)) = output_scope.as_ref() {
                    self.queue_action_controls_for_snapshot_orders(
                        &raw_maps,
                        &stored_sequences,
                        &queue_states,
                        &order_controls,
                        &canonical_apparatuses,
                        holds,
                        ids,
                        apparatus_ids,
                        &projections,
                    )
                    .await
                } else {
                    self.queue_action_controls_for_snapshot(
                        &raw_maps,
                        &stored_sequences,
                        &queue_states,
                        &order_controls,
                        &canonical_apparatuses,
                        holds,
                        &projections,
                    )
                    .await
                }
            },
            self.store.queue_snapshot_logs_for_orders(&order_ids),
            self.store.production_order_lifecycles(&order_ids),
        )?;

        let output_maps = output_scope.as_ref().map(|(ids, _)| {
            raw_maps
                .iter()
                .filter(|map| ids.contains(&map.id))
                .cloned()
                .collect::<Vec<_>>()
        });
        let mut stage_states = stage_states_for_snapshot(
            &projections,
            output_scope.as_ref().map(|(ids, _)| ids),
            &queue_action_controls,
            &queue_logs_by_order,
            &queue_states,
        );
        for stage in &stage_order_controls {
            if stage.state == OrderControlState::Frozen {
                stage_states.entry(stage.order_id.clone()).or_default()
                    .insert(stage.stage_node_id.clone(), "frozen".to_string());
            }
        }
        for (apparatus, controls) in &queue_action_controls {
            let states = queue_states.entry(apparatus.clone()).or_default();
            for (order_id, control) in controls {
                states.insert(order_id.clone(), control.state.as_str().to_string());
                if control.state == queue_state::ApparatusQueueOrderState::Frozen {
                    if let Some(sequence) = sequences.get_mut(apparatus) {
                        sequence.retain(|id| id != order_id);
                    }
                }
            }
        }
        let order_statuses =
            Self::order_status_details_from_lifecycles(&order_ids, &lifecycles, &order_controls)?;
        for (apparatus, orders) in &mut visible_order_ids {
            orders.retain(|order_id| {
                if order_statuses.get(order_id).is_some_and(|detail| {
                    matches!(
                        detail.lifecycle_status,
                        ProductionOrderLifecycleStatus::ProductionCompleted
                            | ProductionOrderLifecycleStatus::Closed
                            | ProductionOrderLifecycleStatus::Cancelled
                    )
                }) {
                    return false;
                }
                let Some(projection) = projections_by_order.get(order_id.as_str()) else {
                    return false;
                };
                let map = projection.map();
                let occurrences = projection.stages()
                    .into_iter()
                    .filter(|stage| stage.apparatus_id.as_deref() == Some(apparatus.as_str()))
                    .collect::<Vec<_>>();
                let own_control = queue_action_controls
                    .get(apparatus)
                    .and_then(|controls| controls.get(order_id));
                occurrences.is_empty()
                    || !occurrences.iter().all(|stage| {
                        stage_states
                            .get(order_id)
                            .and_then(|states| states.get(&stage.node_id))
                            .is_some_and(|state| state == "completed")
                            || own_control.is_some_and(|control| {
                                chain::stage_node_ids_match_for_map(
                                    map,
                                    &stage.node_id,
                                    &control.stage_node_id,
                                ) && control
                                    .stage_work
                                    .as_ref()
                                    .is_some_and(|work| work.local_completed)
                            })
                    })
            });
        }
        let mut frozen_orders_by_apparatus =
            Self::frozen_orders_by_apparatus(&order_controls, &queue_logs_by_order);
        for stage in &stage_order_controls {
            if stage.state == OrderControlState::Frozen {
                let records = BTreeMap::from([(stage.order_id.clone(), stage.clone())]);
                for (apparatus, frozen) in Self::frozen_orders_by_apparatus(&records, &queue_logs_by_order) {
                    let target = frozen_orders_by_apparatus.entry(apparatus).or_default();
                    for record in frozen {
                        if !target.iter().any(|c| c.order_id == record.order_id) { target.push(record); }
                    }
                }
            }
        }
        drop(projections_by_order);
        drop(projections);
        let maps = compile_saved_maps_reusing_programs(
            output_maps.unwrap_or(raw_maps),
            previous.map(|snapshot| snapshot.maps.as_slice()),
        );
        let mut snapshot = ProductionMapLiveSnapshot {
            sequence_versions,
            sequence_revisions,
            maps,
            sequences,
            visible_order_ids,
            sequence_positions_by_apparatus: BTreeMap::new(),
            queue_states,
            stage_states,
            queue_policies,
            queue_action_controls,
            order_statuses,
            order_controls,
            stage_order_controls,
            frozen_orders_by_apparatus,
        };
        if let Some((ids, apparatus_ids)) = output_scope {
            // Keep cross-machine route state for selected orders, and omit
            // unrelated queues before putting the derived snapshot in memory.
            for values in snapshot.queue_states.values_mut() {
                values.retain(|order_id, _| ids.contains(order_id));
            }
            snapshot.order_controls.retain(|id, _| ids.contains(id));
            snapshot.stage_order_controls.retain(|c| ids.contains(&c.order_id));
            for values in snapshot.frozen_orders_by_apparatus.values_mut() {
                values.retain(|order| ids.contains(&order.order_id));
            }
            if let Some(scope) = scope {
                let own = |id: &String| {
                    if scope.only_orders {
                        apparatus_ids.contains(id)
                    } else {
                        scope.apparatus.contains(id)
                    }
                };
                snapshot.sequences.retain(|id, _| own(id));
                snapshot.visible_order_ids.retain(|id, _| own(id));
                snapshot.sequence_versions.retain(|id, _| own(id));
                snapshot.sequence_revisions.retain(|id, _| own(id));
                snapshot.frozen_orders_by_apparatus.retain(|id, _| own(id));
            }
        }
        snapshot.sort_free_pick_visible_orders();
        Ok(snapshot)
    }

    fn frozen_orders_by_apparatus(
        order_controls: &BTreeMap<String, OrderControlRecord>,
        logs_by_order: &BTreeMap<String, Vec<ProductionOrderLogEntry>>,
    ) -> BTreeMap<String, Vec<FrozenOrderSnapshot>> {
        let mut result = BTreeMap::new();
        for (order_id, control) in order_controls.iter().filter(|(_, control)| {
            control.state == OrderControlState::Frozen && control.early_close.is_none()
        }) {
            let freeze_log = logs_by_order
                .get(order_id)
                .into_iter()
                .flat_map(|logs| logs.iter())
                .rev()
                .find(|log| {
                    log.action == queue_state::ApparatusQueueAction::Freeze
                        && log.to_state == queue_state::ApparatusQueueOrderState::Frozen
                        && (control.stage_node_id.is_empty() || log.stage_node_id == control.stage_node_id)
                });
            let apparatus = control
                .freeze_request
                .as_ref()
                .map(|request| request.target_apparatus.trim())
                .filter(|apparatus| !apparatus.is_empty())
                .or_else(|| freeze_log.map(|log| log.apparatus.trim()))
                .unwrap_or_default();
            if apparatus.is_empty() {
                continue;
            }
            let frozen_at_unix = control
                .frozen_at_unix
                .or_else(|| freeze_log.map(|log| log.created_at_unix))
                .unwrap_or_default();
            let frozen_by = if control.actor.display_name.trim().is_empty() {
                control.actor.ref_.trim().to_string()
            } else {
                control.actor.display_name.trim().to_string()
            };
            result
                .entry(apparatus.to_string())
                .or_insert_with(Vec::new)
                .push(FrozenOrderSnapshot {
                    order_id: order_id.clone(),
                    apparatus: apparatus.to_string(),
                    issue_note: freeze_log
                        .map(|log| log.issue_note.trim().to_string())
                        .unwrap_or_default(),
                    frozen_at_unix,
                    frozen_by,
                });
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::production_map::MemoryProductionMapStore;

    #[test]
    fn committed_preflight_events_are_published_once_in_either_arrival_order() {
        let service = ProductionMapService::new_for_test(std::sync::Arc::new(
            MemoryProductionMapStore::new(),
        ));
        let mut receiver = service.subscribe_live();
        let clone = service.clone();
        for event_id in ["local-first", "listener-first"] {
            assert!(service.notify_print_preflight_once(
                Some(event_id),
                "order",
                "apparatus:default:bosma_9",
            ));
            assert!(!clone.notify_print_preflight_once(
                Some(event_id),
                "order",
                "apparatus:default:bosma_9",
            ));
            assert!(matches!(
                receiver.try_recv().unwrap(),
                ProductionMapLiveEvent::PrintPreflight { .. }
            ));
            assert!(receiver.try_recv().is_err());
        }
        assert_eq!(service.snapshot_revision(), 2);
        // Memory and old writers have no exact transaction ID. Never suppress
        // them using order/status/timing heuristics.
        assert!(service.notify_print_preflight_once(None, "order", "apparatus"));
        assert!(service.notify_print_preflight_once(None, "order", "apparatus"));
        assert_eq!(service.snapshot_revision(), 4);
    }

    #[test]
    fn concurrent_committed_publishers_share_one_bounded_registry() {
        let service = ProductionMapService::new_for_test(std::sync::Arc::new(
            MemoryProductionMapStore::new(),
        ));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        std::thread::scope(|scope| {
            for _ in 0..2 {
                let service = service.clone();
                let barrier = barrier.clone();
                scope.spawn(move || {
                    barrier.wait();
                    service.notify_print_preflight_once(Some("same-commit"), "order", "apparatus");
                });
            }
        });
        assert_eq!(service.snapshot_revision(), 1);
        let mut cache = CommittedLiveEvents::default();
        for index in 0..=COMMITTED_LIVE_EVENTS_LIMIT {
            assert!(cache.remember(&index.to_string()));
        }
        assert_eq!(cache.order.len(), COMMITTED_LIVE_EVENTS_LIMIT);
        assert_eq!(cache.seen.len(), COMMITTED_LIVE_EVENTS_LIMIT);
        assert!(!cache.remember(&COMMITTED_LIVE_EVENTS_LIMIT.to_string()));
        assert!(cache.remember("0"), "Evicted IDs may publish again safely");
    }

    #[test]
    fn coalesced_preflight_dedup_precedes_scope_selection() {
        let service = ProductionMapService::new_for_test(std::sync::Arc::new(
            MemoryProductionMapStore::new(),
        ));
        let mut receiver = service.subscribe_live();
        service.notify_print_preflight_once(Some("old"), "old-order", "apparatus");
        receiver.try_recv().unwrap();
        let changes = [
            (Some("old"), "old-order", "apparatus"),
            (Some("new"), "new-order", "apparatus"),
        ];
        service.notify_print_preflight_batch(&changes, false);
        assert!(matches!(receiver.try_recv().unwrap(),
            ProductionMapLiveEvent::PrintPreflight { order_id, .. }
                if order_id == "new-order"));
        service.notify_print_preflight_batch(&changes, false);
        assert!(receiver.try_recv().is_err());
        service.notify_print_preflight_batch(&changes, true);
        assert_eq!(
            receiver.try_recv().unwrap(),
            ProductionMapLiveEvent::Invalidate
        );
        service.notify_print_preflight_batch(
            &[
                (Some("another-1"), "a", "apparatus"),
                (Some("another-2"), "b", "apparatus"),
            ],
            false,
        );
        assert_eq!(
            receiver.try_recv().unwrap(),
            ProductionMapLiveEvent::Invalidate
        );
        // Two new transitions of the same order can share one fresh projection.
        service.notify_print_preflight_batch(
            &[
                (Some("same-order-1"), "a", "apparatus"),
                (Some("same-order-2"), "a", "apparatus"),
            ],
            false,
        );
        assert!(matches!(
            receiver.try_recv().unwrap(),
            ProductionMapLiveEvent::PrintPreflight { .. }
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn live_snapshot_cache_reuses_shared_arc_until_notification() {
        let service = ProductionMapService::new_for_test(std::sync::Arc::new(
            MemoryProductionMapStore::new(),
        ));

        let initial = service
            .live_snapshot_shared()
            .await
            .expect("initial snapshot");
        let reused = service
            .live_snapshot_shared()
            .await
            .expect("cached snapshot");
        assert!(std::sync::Arc::ptr_eq(&initial, &reused));

        service.notify_live();
        let refreshed = service
            .live_snapshot_shared()
            .await
            .expect("invalidated snapshot");
        assert!(!std::sync::Arc::ptr_eq(&initial, &refreshed));
    }
}
