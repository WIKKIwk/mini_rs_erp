use std::collections::BTreeMap;

use super::progress::QolipLineage;
use super::{
    ApparatusQueueActionEvent, OpeningWipBatch, OrderControlRecord, OrderProgressBatch,
    OrderProgressEvent, OrderRunSession, ProductionMapDefinition,
};

pub(super) struct QueueProgressRecords {
    pub(super) session: Option<OrderRunSession>,
    pub(super) progress_event: Option<OrderProgressEvent>,
    pub(super) progress_batch: Option<OrderProgressBatch>,
    pub(super) progress_batches: Vec<OrderProgressBatch>,
    pub(super) progress_batch_updates: Vec<OrderProgressBatch>,
    pub(super) opening_wip_batch_updates: Vec<OpeningWipBatch>,
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
    pub(super) opening_wip_batch_updates: Vec<OpeningWipBatch>,
    pub(super) material_scan_skipped: bool,
    pub(super) claimed_alternative_map: Option<ClaimedAlternativeMapUpdate>,
    pub(super) order_control_update: Option<OrderControlRecord>,
}

#[derive(Clone)]
pub(super) struct ClaimedAlternativeMapUpdate {
    pub(super) updated: ProductionMapDefinition,
}

impl PreparedApparatusQueueAction {
    pub fn progress_output_batches(&self) -> &[OrderProgressBatch] {
        if self.progress_batches.is_empty() {
            self.progress_batch.as_slice()
        } else {
            &self.progress_batches
        }
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
            session.payload_json["qolip_lock_owner"] = serde_json::Value::Bool(true);
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
