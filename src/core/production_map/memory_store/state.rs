use std::collections::BTreeMap;

use std::sync::atomic::AtomicBool;

use tokio::sync::RwLock;

use super::super::*;

#[cfg(test)]
pub struct MemoryProductionMapStore {
    pub(super) maps: RwLock<BTreeMap<String, ProductionMapDefinition>>,
    pub(super) sequences: RwLock<BTreeMap<String, Vec<String>>>,
    pub(super) apparatus_downtimes: RwLock<BTreeMap<String, ApparatusDowntime>>,
    pub(super) apparatus_schedule_reservations:
        RwLock<BTreeMap<String, ApparatusScheduleReservation>>,
    pub(super) queue_states: RwLock<BTreeMap<String, BTreeMap<String, String>>>,
    pub(super) order_controls: RwLock<BTreeMap<String, OrderControlRecord>>,
    pub(super) order_freeze_requests: RwLock<BTreeMap<String, OrderFreezeAuditRecord>>,
    pub(super) queue_events: RwLock<Vec<ApparatusQueueActionEvent>>,
    pub(super) order_run_sessions: RwLock<BTreeMap<String, OrderRunSession>>,
    pub(super) order_progress_events: RwLock<Vec<OrderProgressEvent>>,
    pub(super) order_progress_batches: RwLock<BTreeMap<String, OrderProgressBatch>>,
    pub(super) progress_batch_corrections: RwLock<Vec<ProgressBatchCorrectionRecord>>,
    pub(super) laminatsiya_astatka_reports: RwLock<Vec<LaminatsiyaAstatkaReport>>,
    pub(super) rezka_astatka_reports: RwLock<Vec<RezkaAstatkaReport>>,
    pub(super) finished_goods_stock: RwLock<BTreeMap<String, FinishedGoodsStockEntry>>,
    pub(super) material_assignments: RwLock<BTreeMap<String, RawMaterialAssignment>>,
    pub(super) returned_paint_requests:
        RwLock<BTreeMap<String, crate::core::returned_paint::ReturnedPaintRequest>>,
    pub(super) apparatus_transfers: RwLock<BTreeMap<String, ProductionMapApparatusTransferRecord>>,
    pub(super) fail_next_queue_progress_commit: AtomicBool,
}

#[cfg(test)]
impl MemoryProductionMapStore {
    pub fn new() -> Self {
        Self {
            maps: RwLock::new(BTreeMap::new()),
            sequences: RwLock::new(BTreeMap::new()),
            apparatus_downtimes: RwLock::new(BTreeMap::new()),
            apparatus_schedule_reservations: RwLock::new(BTreeMap::new()),
            queue_states: RwLock::new(BTreeMap::new()),
            order_controls: RwLock::new(BTreeMap::new()),
            order_freeze_requests: RwLock::new(BTreeMap::new()),
            queue_events: RwLock::new(Vec::new()),
            order_run_sessions: RwLock::new(BTreeMap::new()),
            order_progress_events: RwLock::new(Vec::new()),
            order_progress_batches: RwLock::new(BTreeMap::new()),
            progress_batch_corrections: RwLock::new(Vec::new()),
            laminatsiya_astatka_reports: RwLock::new(Vec::new()),
            rezka_astatka_reports: RwLock::new(Vec::new()),
            finished_goods_stock: RwLock::new(BTreeMap::new()),
            material_assignments: RwLock::new(BTreeMap::new()),
            returned_paint_requests: RwLock::new(BTreeMap::new()),
            apparatus_transfers: RwLock::new(BTreeMap::new()),
            fail_next_queue_progress_commit: AtomicBool::new(false),
        }
    }

    pub fn fail_next_queue_progress_commit(&self) {
        self.fail_next_queue_progress_commit
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub async fn returned_paint_request(
        &self,
        request_id: &str,
    ) -> Option<crate::core::returned_paint::ReturnedPaintRequest> {
        self.returned_paint_requests
            .read()
            .await
            .get(request_id.trim())
            .cloned()
    }

    pub async fn progress_batch_correction_records(&self) -> Vec<ProgressBatchCorrectionRecord> {
        self.progress_batch_corrections.read().await.clone()
    }
}

#[cfg(test)]
impl Default for MemoryProductionMapStore {
    fn default() -> Self {
        Self::new()
    }
}
