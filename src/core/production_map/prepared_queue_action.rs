use std::collections::BTreeMap;

use super::progress::QolipLineage;
use super::{
    ApparatusQueueActionEvent, OpeningWipBatch, OrderControlRecord, OrderProgressBatch,
    OrderProgressEvent, OrderRunSession, ProductionMapDefinition, ProductionQrRawMaterial,
    ProductionQrSessionResources,
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
    pub(super) claimed_alternative_map: Option<ProductionMapDefinition>,
    pub(super) order_control_update: Option<OrderControlRecord>,
    pub(super) print_preflight_hold_id: Option<String>,
    pub(super) print_preflight_cancel_hold_id: Option<String>,
}

impl PreparedApparatusQueueAction {
    pub(crate) fn attach_active_paddon(&mut self) {
        if !self.progress_output_batches().is_empty() {
            self.event.payload_json["use_active_paddon"] = serde_json::json!(true);
        }
    }
    pub(crate) fn attach_print_preflight_hold_id(&mut self, hold_id: &str) {
        let hold_id = hold_id.trim();
        if !hold_id.is_empty() {
            self.print_preflight_hold_id = Some(hold_id.to_string());
        }
    }

    /// Only newly produced batches are assigned. Replays, issues, and updates
    /// to previously printed rolls must never change pallet membership.
    #[cfg(test)]
    pub(crate) fn attach_output_paddon(&mut self, code: &str) {
        let code = code.trim();
        if code.is_empty() || self.progress_output_batches().is_empty() {
            return;
        }
        self.event.payload_json["output_paddon_code"] = serde_json::json!(code);
        if let Some(event) = &mut self.progress_event {
            event.payload_json["output_paddon_code"] = serde_json::json!(code);
        }
    }

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

    pub(crate) fn attach_start_materials(&mut self, materials: Vec<ProductionQrRawMaterial>) {
        if let Some(session) = &mut self.session {
            let mut resources = ProductionQrSessionResources::for_session(session);
            resources.raw_materials = materials;
            resources.raw_materials_available = true;
            resources.write_to_session(session);
        }
    }

    pub fn attach_qolip_codes(&mut self, qolip_codes: &[String]) {
        self.attach_qolip_set(qolip_codes, "");
    }

    pub fn attach_qolip_set(&mut self, qolip_codes: &[String], set_id: &str) {
        let Some(mut lineage) = QolipLineage::from_codes(qolip_codes) else {
            return;
        };
        lineage.qolip_set_id = set_id.to_string();
        if let Some(session) = &mut self.session {
            if session.qolip_reacquisition_required() {
                self.event.payload_json["qolip_reacquired_after_freeze"] = serde_json::json!(true);
            }
            lineage.write_to_payload(&mut session.payload_json);
            session.payload_json["qolip_lock_owner"] = serde_json::Value::Bool(true);
            session.payload_json["qolip_released_on_freeze"] = serde_json::json!(false);
            let mut resources = ProductionQrSessionResources::for_session(session);
            resources.qolip_codes = lineage.qolip_codes.clone();
            resources.qolip_available = true;
            resources.write_to_session(session);
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
