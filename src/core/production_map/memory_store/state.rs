use std::collections::BTreeMap;

use tokio::sync::RwLock;

use super::super::*;

#[cfg(test)]
pub struct MemoryProductionMapStore {
    pub(super) maps: RwLock<BTreeMap<String, ProductionMapDefinition>>,
    pub(super) sequences: RwLock<BTreeMap<String, Vec<String>>>,
    pub(super) queue_states: RwLock<BTreeMap<String, BTreeMap<String, String>>>,
    pub(super) queue_policies: RwLock<BTreeMap<String, ApparatusQueuePolicy>>,
    pub(super) queue_events: RwLock<Vec<ApparatusQueueActionEvent>>,
    pub(super) order_run_sessions: RwLock<BTreeMap<String, OrderRunSession>>,
    pub(super) order_progress_events: RwLock<Vec<OrderProgressEvent>>,
    pub(super) order_progress_batches: RwLock<BTreeMap<String, OrderProgressBatch>>,
    pub(super) finished_goods_stock: RwLock<BTreeMap<String, FinishedGoodsStockEntry>>,
    pub(super) material_rules: RwLock<BTreeMap<String, ApparatusMaterialRule>>,
    pub(super) material_assignments: RwLock<BTreeMap<String, RawMaterialAssignment>>,
}

#[cfg(test)]
impl MemoryProductionMapStore {
    pub fn new() -> Self {
        Self {
            maps: RwLock::new(BTreeMap::new()),
            sequences: RwLock::new(BTreeMap::new()),
            queue_states: RwLock::new(BTreeMap::new()),
            queue_policies: RwLock::new(BTreeMap::new()),
            queue_events: RwLock::new(Vec::new()),
            order_run_sessions: RwLock::new(BTreeMap::new()),
            order_progress_events: RwLock::new(Vec::new()),
            order_progress_batches: RwLock::new(BTreeMap::new()),
            finished_goods_stock: RwLock::new(BTreeMap::new()),
            material_rules: RwLock::new(BTreeMap::new()),
            material_assignments: RwLock::new(BTreeMap::new()),
        }
    }
}

#[cfg(test)]
impl Default for MemoryProductionMapStore {
    fn default() -> Self {
        Self::new()
    }
}
