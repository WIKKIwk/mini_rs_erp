use super::*;

use std::collections::BTreeMap;

use super::apparatus::visible_order_ids_by_apparatus;
use super::progress::effective_apparatus_queue_policy_record;
use super::service_maps::compile_saved_maps;
use serde::Serialize;
use tokio::sync::{Mutex, OwnedMutexGuard, broadcast};

const LIVE_NOTIFY_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize)]
pub struct ProductionMapLiveSnapshot {
    pub maps: Vec<ProductionMapSaved>,
    pub sequences: BTreeMap<String, Vec<String>>,
    pub visible_order_ids: BTreeMap<String, Vec<String>>,
    pub queue_states: BTreeMap<String, BTreeMap<String, String>>,
    pub queue_policies: Vec<ApparatusQueuePolicyRecord>,
    pub queue_action_controls:
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
    pub order_statuses: BTreeMap<String, ProductionOrderStatusDetail>,
    pub order_controls: BTreeMap<String, OrderControlRecord>,
}

#[derive(Clone)]
pub(crate) struct ProductionSnapshotContext {
    pub(crate) maps: Vec<ProductionMapSaved>,
    pub(crate) sequences: BTreeMap<String, Vec<String>>,
    pub(crate) visible_order_ids: BTreeMap<String, Vec<String>>,
    pub(crate) queue_states: BTreeMap<String, BTreeMap<String, String>>,
    pub(crate) queue_policies: Vec<ApparatusQueuePolicyRecord>,
    pub(crate) queue_action_controls: BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
    pub(crate) order_statuses: BTreeMap<String, ProductionOrderStatusDetail>,
    pub(crate) order_controls: BTreeMap<String, OrderControlRecord>,
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
        }
    }
}

#[derive(Clone)]
pub struct ProductionMapService {
    pub(super) store: std::sync::Arc<dyn ProductionMapStorePort>,
    live_notify: broadcast::Sender<()>,
    queue_action_lock: std::sync::Arc<Mutex<()>>,
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
    pub(super) previous: ProductionMapDefinition,
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
        let mut normalized = Vec::new();
        for code in qolip_codes {
            let code = code.trim();
            if code.is_empty()
                || normalized
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(code))
            {
                continue;
            }
            normalized.push(code.to_string());
        }
        if normalized.is_empty() {
            return;
        }
        if let Some(session) = &mut self.session {
            if !session.payload_json.is_object() {
                session.payload_json = serde_json::json!({});
            }
            session.payload_json["qolip_code"] = serde_json::json!(normalized[0]);
            session.payload_json["qolip_codes"] = serde_json::json!(normalized);
        }
    }

}

impl ProductionMapService {
    pub fn new(store: std::sync::Arc<dyn ProductionMapStorePort>) -> Self {
        let (live_notify, _) = broadcast::channel(LIVE_NOTIFY_CAPACITY);
        Self {
            store,
            live_notify,
            queue_action_lock: std::sync::Arc::new(Mutex::new(())),
        }
    }

    pub(crate) async fn queue_action_guard(&self) -> OwnedMutexGuard<()> {
        self.queue_action_lock.clone().lock_owned().await
    }

    pub fn subscribe_live(&self) -> broadcast::Receiver<()> {
        self.live_notify.subscribe()
    }

    pub fn notify_live(&self) {
        let _ = self.live_notify.send(());
    }

    pub async fn live_snapshot(&self) -> Result<ProductionMapLiveSnapshot, ProductionMapError> {
        Ok(self.production_snapshot_context().await?.into())
    }

    pub(crate) async fn production_snapshot_context(
        &self,
    ) -> Result<ProductionSnapshotContext, ProductionMapError> {
        let raw_maps = self.store.maps().await?;
        let maps = compile_saved_maps(raw_maps.clone());
        let stored_sequences = self.store.apparatus_sequences().await?;
        let visible_order_ids = visible_order_ids_by_apparatus(&raw_maps);
        let queue_states = self.store.apparatus_queue_states().await?;
        let policies = self.store.apparatus_queue_policies().await?;
        let order_controls = self.store.order_control_states().await?;
        let sequences = Self::effective_apparatus_sequences_for_maps(&raw_maps, &stored_sequences);
        let queue_policies = policies
            .iter()
            .map(|(apparatus, policy)| effective_apparatus_queue_policy_record(apparatus, *policy))
            .collect();
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
            .order_status_details_for_snapshot(&maps, &queue_states)
            .await?;

        Ok(ProductionSnapshotContext {
            maps,
            sequences,
            visible_order_ids,
            queue_states,
            queue_policies,
            queue_action_controls,
            order_statuses,
            order_controls,
        })
    }
}
