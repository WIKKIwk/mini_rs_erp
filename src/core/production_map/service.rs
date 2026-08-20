use super::*;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

use super::apparatus::visible_order_ids_by_apparatus;
use super::apparatus_resolver::{
    CanonicalApparatusResolver, UnavailableCanonicalApparatusResolver,
};
use super::progress::QolipLineage;
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
            queue_policies: context.queue_policies,
            queue_action_controls: context.queue_action_controls,
            order_statuses: context.order_statuses,
            order_controls: context.order_controls,
            frozen_orders_by_apparatus: context.frozen_orders_by_apparatus,
        }
    }
}

#[derive(Clone)]
pub struct ProductionMapService {
    pub(super) store: std::sync::Arc<dyn ProductionMapStorePort>,
    pub(super) apparatus_resolver: std::sync::Arc<dyn CanonicalApparatusResolver>,
    live_notify: broadcast::Sender<()>,
    queue_action_lock: std::sync::Arc<Mutex<()>>,
    snapshot_cache: std::sync::Arc<ProductionSnapshotCache>,
}

pub(super) struct QueueProgressRecords {
    pub(super) session: Option<OrderRunSession>,
    pub(super) progress_event: Option<OrderProgressEvent>,
    pub(super) progress_batch: Option<OrderProgressBatch>,
    pub(super) progress_batches: Vec<OrderProgressBatch>,
    pub(super) progress_batch_updates: Vec<OrderProgressBatch>,
}

pub struct PreparedApparatusQueueAction {
    pub(super) apparatus: String,
    pub(super) states: BTreeMap<String, String>,
    pub(super) sequence_updates: BTreeMap<String, Vec<String>>,
    pub(super) event: ApparatusQueueActionEvent,
    pub(super) session: Option<OrderRunSession>,
    pub(super) progress_event: Option<OrderProgressEvent>,
    pub(super) progress_batch: Option<OrderProgressBatch>,
    pub(super) progress_batches: Vec<OrderProgressBatch>,
    pub(super) progress_batch_updates: Vec<OrderProgressBatch>,
    pub(super) material_scan_skipped: bool,
    pub(super) claimed_alternative_map: Option<ClaimedAlternativeMapUpdate>,
    pub(super) order_control_update: Option<OrderControlRecord>,
}

#[derive(Clone)]
pub(super) struct ClaimedAlternativeMapUpdate {
    pub(super) updated: ProductionMapDefinition,
}

impl PreparedApparatusQueueAction {
    pub fn progress_batch(&self) -> Option<&OrderProgressBatch> {
        self.progress_batch.as_ref()
    }

    pub fn progress_batches(&self) -> &[OrderProgressBatch] {
        &self.progress_batches
    }

    pub fn material_scan_skipped(&self) -> bool {
        self.material_scan_skipped
    }

    pub fn attach_qolip_codes(&mut self, qolip_codes: &[String]) {
        let Some(lineage) = QolipLineage::from_codes(qolip_codes) else {
            return;
        };
        if let Some(session) = &mut self.session {
            lineage.write_to_payload(&mut session.payload_json);
        }
        lineage.write_to_payload(&mut self.event.payload_json);
        if let Some(progress_event) = &mut self.progress_event {
            lineage.write_to_payload(&mut progress_event.payload_json);
        }
        if let Some(progress_batch) = &mut self.progress_batch {
            lineage.write_to_payload(&mut progress_batch.payload_json);
        }
        for progress_batch in &mut self.progress_batches {
            lineage.write_to_payload(&mut progress_batch.payload_json);
        }
        for progress_batch in &mut self.progress_batch_updates {
            lineage.write_to_payload(&mut progress_batch.payload_json);
        }
    }
}

impl ProductionMapService {
    pub fn new(store: std::sync::Arc<dyn ProductionMapStorePort>) -> Self {
        let (live_notify, _) = broadcast::channel(LIVE_NOTIFY_CAPACITY);
        Self {
            store,
            apparatus_resolver: std::sync::Arc::new(UnavailableCanonicalApparatusResolver),
            live_notify,
            queue_action_lock: std::sync::Arc::new(Mutex::new(())),
            snapshot_cache: std::sync::Arc::new(ProductionSnapshotCache::default()),
        }
    }

    pub fn with_canonical_apparatus_resolver(
        mut self,
        resolver: std::sync::Arc<dyn CanonicalApparatusResolver>,
    ) -> Self {
        self.apparatus_resolver = resolver;
        self
    }

    /// Resolve live apparatus configuration by immutable canonical identity.
    /// Missing or invalid configuration returns `StoreFailed`; callers cannot
    /// continue with a title/name-derived fallback.
    pub(crate) async fn resolve_canonical_apparatus(
        &self,
        apparatus_id: &crate::core::apparatus_standard::ApparatusId,
    ) -> Result<
        std::sync::Arc<crate::core::apparatus_standard::CanonicalApparatus>,
        ProductionMapError,
    > {
        self.apparatus_resolver
            .resolve(apparatus_id)
            .await?
            .ok_or(ProductionMapError::StoreFailed)
    }

    pub(crate) async fn resolve_canonical_apparatus_text(
        &self,
        value: &str,
    ) -> Result<
        std::sync::Arc<crate::core::apparatus_standard::CanonicalApparatus>,
        ProductionMapError,
    > {
        let id = crate::core::apparatus_standard::ApparatusId::new(value.trim().to_string())
            .map_err(|_| ProductionMapError::StoreFailed)?;
        self.resolve_canonical_apparatus(&id).await
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
        let queue_states = self.store.apparatus_queue_states().await?;
        let policies = self.store.apparatus_queue_policies().await?;
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
        for (apparatus_id, policy) in &policies {
            let canonical = self.resolve_canonical_apparatus(apparatus_id).await?;
            queue_policies.push(effective_apparatus_queue_policy_record(&canonical, *policy));
        }
        let queue_action_controls = self
            .queue_action_controls_for_snapshot(
                &raw_maps,
                &stored_sequences,
                &queue_states,
                &policies,
                &order_controls,
            )
            .await?;
        let order_statuses = self
            .order_status_details_for_snapshot(&maps, &queue_states, &order_controls)
            .await?;
        let frozen_orders_by_apparatus = self.frozen_orders_by_apparatus(&order_controls).await?;

        Ok(ProductionSnapshotContext {
            maps,
            sequences,
            visible_order_ids,
            queue_states,
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
        let service =
            ProductionMapService::new(std::sync::Arc::new(MemoryProductionMapStore::new()));

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
