use super::*;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

use super::apparatus::visible_order_ids_by_apparatus;
use super::apparatus_resolver::CanonicalApparatusResolver;
pub use super::prepared_queue_action::PreparedApparatusQueueAction;
pub(super) use super::prepared_queue_action::{ClaimedAlternativeMapUpdate, QueueProgressRecords};
use super::progress::effective_apparatus_queue_policy_record;
use super::service_maps::compile_saved_maps;
use serde::Serialize;
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock, broadcast};

const LIVE_NOTIFY_CAPACITY: usize = 256;

#[derive(Default)]
struct ProductionSnapshotCache {
    revision: AtomicU64,
    snapshot: RwLock<Option<CachedProductionSnapshot>>,
    rebuild_lock: Mutex<()>,
}

struct CachedProductionSnapshot {
    revision: u64,
    snapshot: std::sync::Arc<ProductionMapLiveSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProductionMapLiveSnapshot {
    pub maps: Vec<ProductionMapSaved>,
    pub sequences: BTreeMap<String, Vec<String>>,
    pub visible_order_ids: BTreeMap<String, Vec<String>>,
    pub queue_states: BTreeMap<String, BTreeMap<String, String>>,
    pub stage_states: BTreeMap<String, BTreeMap<String, String>>,
    pub queue_policies: Vec<ApparatusQueuePolicyRecord>,
    pub queue_action_controls: BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
    pub order_statuses: BTreeMap<String, ProductionOrderStatusDetail>,
    pub order_controls: BTreeMap<String, OrderControlRecord>,
    pub frozen_orders_by_apparatus: BTreeMap<String, Vec<FrozenOrderSnapshot>>,
}

#[derive(Clone)]
pub(crate) struct ProductionSnapshotContext {
    pub(crate) maps: Vec<ProductionMapSaved>,
    pub(crate) sequences: BTreeMap<String, Vec<String>>,
    pub(crate) visible_order_ids: BTreeMap<String, Vec<String>>,
    pub(crate) queue_states: BTreeMap<String, BTreeMap<String, String>>,
    pub(crate) stage_states: BTreeMap<String, BTreeMap<String, String>>,
    pub(crate) queue_policies: Vec<ApparatusQueuePolicyRecord>,
    pub(crate) queue_action_controls:
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
    pub(crate) order_statuses: BTreeMap<String, ProductionOrderStatusDetail>,
    pub(crate) order_controls: BTreeMap<String, OrderControlRecord>,
    pub(crate) frozen_orders_by_apparatus: BTreeMap<String, Vec<FrozenOrderSnapshot>>,
}

impl From<ProductionSnapshotContext> for ProductionMapLiveSnapshot {
    fn from(context: ProductionSnapshotContext) -> Self {
        Self {
            maps: context.maps,
            sequences: context.sequences,
            visible_order_ids: context.visible_order_ids,
            queue_states: context.queue_states,
            stage_states: context.stage_states,
            queue_policies: context.queue_policies,
            queue_action_controls: context.queue_action_controls,
            order_statuses: context.order_statuses,
            order_controls: context.order_controls,
            frozen_orders_by_apparatus: context.frozen_orders_by_apparatus,
        }
    }
}

fn stage_states_for_snapshot(
    maps: &[ProductionMapDefinition],
    controls: &BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
    logs_by_order: &BTreeMap<String, Vec<ProductionOrderLogEntry>>,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    for map in maps {
        let order_id = map.id.trim();
        if order_id.is_empty() {
            continue;
        }
        let stages = chain::linear_work_stages(map)
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

        let apparatuses = stages
            .iter()
            .filter_map(|stage| stage.apparatus_id.as_deref())
            .map(str::trim)
            .filter(|apparatus| !apparatus.is_empty())
            .collect::<BTreeSet<_>>();
        for apparatus in apparatuses {
            let occurrences = stages
                .iter()
                .filter(|stage| {
                    stage.apparatus_id.as_deref().is_some_and(|candidate| {
                        super::types::apparatus_ids_match(candidate, apparatus)
                    })
                })
                .collect::<Vec<_>>();
            let control = controls.iter().find_map(|(candidate, orders)| {
                super::types::apparatus_ids_match(candidate, apparatus)
                    .then(|| orders.get(order_id))
                    .flatten()
            });
            let Some(control) = control else {
                continue;
            };
            if occurrences.len() == 1 {
                states.insert(
                    occurrences[0].node_id.trim().to_string(),
                    control.state.as_str().to_string(),
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
                    states.insert(stage_node_id, control.state.as_str().to_string());
                } else {
                    states
                        .entry(stage_node_id)
                        .or_insert_with(|| "pending".to_string());
                }
            }
        }
        for stage in stages {
            states
                .entry(stage.node_id.trim().to_string())
                .or_insert_with(|| "pending".to_string());
        }
        result.insert(order_id.to_string(), states);
    }
    result
}

#[derive(Clone)]
pub struct ProductionMapService {
    pub(super) store: std::sync::Arc<dyn ProductionMapStorePort>,
    pub(super) apparatus_resolver: std::sync::Arc<dyn CanonicalApparatusResolver>,
    live_notify: broadcast::Sender<()>,
    queue_action_lock: std::sync::Arc<Mutex<()>>,
    snapshot_cache: std::sync::Arc<ProductionSnapshotCache>,
}

impl ProductionMapService {
    pub fn new(
        store: std::sync::Arc<dyn ProductionMapStorePort>,
        apparatus_resolver: std::sync::Arc<dyn CanonicalApparatusResolver>,
    ) -> Self {
        let (live_notify, _) = broadcast::channel(LIVE_NOTIFY_CAPACITY);
        Self {
            store,
            apparatus_resolver,
            live_notify,
            queue_action_lock: std::sync::Arc::new(Mutex::new(())),
            snapshot_cache: std::sync::Arc::new(ProductionSnapshotCache::default()),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(store: std::sync::Arc<dyn ProductionMapStorePort>) -> Self {
        Self::new(
            store,
            std::sync::Arc::new(super::TestCanonicalApparatusResolver::standard()),
        )
    }

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

    pub(crate) async fn queue_action_guard(&self) -> OwnedMutexGuard<()> {
        self.queue_action_lock.clone().lock_owned().await
    }

    pub fn subscribe_live(&self) -> broadcast::Receiver<()> {
        self.live_notify.subscribe()
    }

    pub fn notify_live(&self) {
        self.snapshot_cache.revision.fetch_add(1, Ordering::AcqRel);
        let _ = self.live_notify.send(());
    }

    pub async fn live_snapshot(&self) -> Result<ProductionMapLiveSnapshot, ProductionMapError> {
        loop {
            let revision = self.snapshot_cache.revision.load(Ordering::Acquire);
            {
                let cached = self.snapshot_cache.snapshot.read().await;
                if let Some(entry) = cached.as_ref()
                    && entry.revision == revision
                {
                    return Ok(entry.snapshot.as_ref().clone());
                }
            }

            let _rebuild_guard = self.snapshot_cache.rebuild_lock.lock().await;
            let revision = self.snapshot_cache.revision.load(Ordering::Acquire);
            {
                let cached = self.snapshot_cache.snapshot.read().await;
                if let Some(entry) = cached.as_ref()
                    && entry.revision == revision
                {
                    return Ok(entry.snapshot.as_ref().clone());
                }
            }

            let snapshot: ProductionMapLiveSnapshot =
                self.build_production_snapshot_context().await?.into();
            let latest_revision = self.snapshot_cache.revision.load(Ordering::Acquire);
            if latest_revision != revision {
                continue;
            }

            let snapshot = std::sync::Arc::new(snapshot);
            let result = snapshot.as_ref().clone();
            *self.snapshot_cache.snapshot.write().await =
                Some(CachedProductionSnapshot { revision, snapshot });
            return Ok(result);
        }
    }

    async fn build_production_snapshot_context(
        &self,
    ) -> Result<ProductionSnapshotContext, ProductionMapError> {
        let raw_maps = self.store.maps().await?;
        let maps = compile_saved_maps(raw_maps.clone());
        let stored_sequences = self.store.apparatus_sequences().await?;
        let visible_order_ids = visible_order_ids_by_apparatus(&raw_maps);
        let mut queue_states = self.store.apparatus_queue_states().await?;
        let order_controls = self.store.order_control_states().await?;
        let frozen_order_ids = order_controls
            .iter()
            .filter_map(|(id, control)| {
                (control.state == OrderControlState::Frozen).then_some(id.clone())
            })
            .collect::<BTreeSet<_>>();
        let sequences = Self::effective_apparatus_sequences_for_maps(
            &raw_maps,
            &stored_sequences,
            &frozen_order_ids,
        );
        let mut queue_policies = Vec::new();
        for canonical in self.active_canonical_apparatuses().await? {
            queue_policies.push(effective_apparatus_queue_policy_record(&canonical));
        }
        let queue_action_controls = self
            .queue_action_controls_for_snapshot(
                &raw_maps,
                &stored_sequences,
                &queue_states,
                &order_controls,
            )
            .await?;
        for (apparatus, controls) in &queue_action_controls {
            let states = queue_states.entry(apparatus.clone()).or_default();
            for (order_id, control) in controls {
                states.insert(order_id.clone(), control.state.as_str().to_string());
            }
        }
        let order_ids = raw_maps
            .iter()
            .map(|map| map.id.trim().to_string())
            .filter(|order_id| !order_id.is_empty())
            .collect::<Vec<_>>();
        let queue_logs_by_order = self.store.queue_action_logs_for_orders(&order_ids).await?;
        let stage_states =
            stage_states_for_snapshot(&raw_maps, &queue_action_controls, &queue_logs_by_order);
        let order_statuses = self
            .order_status_details_for_snapshot(&maps, &order_controls)
            .await?;
        let frozen_orders_by_apparatus = self.frozen_orders_by_apparatus(&order_controls).await?;

        Ok(ProductionSnapshotContext {
            maps,
            sequences,
            visible_order_ids,
            queue_states,
            stage_states,
            queue_policies,
            queue_action_controls,
            order_statuses,
            order_controls,
            frozen_orders_by_apparatus,
        })
    }

    async fn frozen_orders_by_apparatus(
        &self,
        order_controls: &BTreeMap<String, OrderControlRecord>,
    ) -> Result<BTreeMap<String, Vec<FrozenOrderSnapshot>>, ProductionMapError> {
        let frozen_order_ids = order_controls
            .iter()
            .filter_map(|(order_id, control)| {
                (control.state == OrderControlState::Frozen).then_some(order_id.clone())
            })
            .collect::<Vec<_>>();
        let logs_by_order = self
            .store
            .queue_action_logs_for_orders(&frozen_order_ids)
            .await?;
        let mut result = BTreeMap::new();
        for order_id in frozen_order_ids {
            let Some(control) = order_controls.get(&order_id) else {
                continue;
            };
            let logs = logs_by_order.get(&order_id).cloned().unwrap_or_default();
            let freeze_log = logs.iter().rev().find(|log| {
                log.action == queue_state::ApparatusQueueAction::Freeze
                    && log.to_state == queue_state::ApparatusQueueOrderState::Frozen
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
                    order_id,
                    apparatus: apparatus.to_string(),
                    issue_note: freeze_log
                        .map(|log| log.issue_note.trim().to_string())
                        .unwrap_or_default(),
                    frozen_at_unix,
                    frozen_by,
                });
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::production_map::MemoryProductionMapStore;

    async fn cached_snapshot(
        service: &ProductionMapService,
    ) -> std::sync::Arc<ProductionMapLiveSnapshot> {
        service
            .snapshot_cache
            .snapshot
            .read()
            .await
            .as_ref()
            .expect("snapshot cache entry")
            .snapshot
            .clone()
    }

    #[tokio::test]
    async fn live_snapshot_cache_reuses_until_notification() {
        let service = ProductionMapService::new_for_test(std::sync::Arc::new(
            MemoryProductionMapStore::new(),
        ));

        service.live_snapshot().await.expect("initial snapshot");
        let initial = cached_snapshot(&service).await;

        service.live_snapshot().await.expect("cached snapshot");
        let reused = cached_snapshot(&service).await;
        assert!(std::sync::Arc::ptr_eq(&initial, &reused));

        service.notify_live();
        service.live_snapshot().await.expect("invalidated snapshot");
        let refreshed = cached_snapshot(&service).await;
        assert!(!std::sync::Arc::ptr_eq(&initial, &refreshed));
    }
}
