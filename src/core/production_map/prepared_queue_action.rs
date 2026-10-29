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
    pub(crate) fn attach_automatic_qolip_confirmation(&mut self, confirmation: &super::AutomaticQolipConfirmation) {
        let value = serde_json::to_value(confirmation).expect("serializable mold confirmation");
        self.event.payload_json["qolip_confirmation"] = value.clone();
        if let Some(session) = &mut self.session {
            session.payload_json["qolip_confirmation"] = value;
        }
    }
    /// Build the ordinary Pause/Complete write for an atomic layer return.
    pub(crate) fn into_lamination_report_queue_write(mut self) -> super::QueueActionProgressWrite {
        let completing = self.event.action == super::queue_state::ApparatusQueueAction::Complete;
        // The existing Astatka report owns these counters. In the zero-output
        // handoff they validated Pause, but must not become a second report in
        // the ordinary progress event.
        if !completing && let Some(event) = &mut self.progress_event {
            event.lamination_print_leftover_rolls = None;
            event.lamination_film_leftover_rolls = None;
            event.total_waste = None;
            for field in [
                "lamination_print_leftover_rolls",
                "lamination_film_leftover_rolls",
                "total_waste",
            ] {
                event.payload_json[field] = serde_json::Value::Null;
            }
        }
        if !completing && let Some(session) = &mut self.session {
            for field in [
                "lamination_print_leftover_rolls",
                "lamination_film_leftover_rolls",
                "total_waste",
            ] {
                session.payload_json[field] = serde_json::Value::Null;
            }
        }
        super::QueueActionProgressWrite {
            apparatus: self.apparatus,
            map_update: self.claimed_alternative_map,
            states: self.states,
            sequence_updates: self.sequence_updates,
            event: self.event,
            session: self.session,
            progress_event: self.progress_event,
            progress_batch: self.progress_batch,
            progress_batches: self.progress_batches,
            progress_batch_updates: self.progress_batch_updates,
            opening_wip_batch_updates: self.opening_wip_batch_updates,
            raw_material_stock_transitions: Vec::new(),
            qolip_checkouts: Vec::new(),
            returned_paint_report: None,
            order_control_update: self.order_control_update,
            schedule_reservation_status: Some(if completing {
                super::ApparatusScheduleStatus::Completed
            } else {
                super::ApparatusScheduleStatus::Paused
            }),
            print_preflight_hold_id: self.print_preflight_hold_id,
            print_preflight_cancel_hold_id: self.print_preflight_cancel_hold_id,
        }
    }
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

    /// Raw materials belong to the station's work across input rolls. Only
    /// the server-resolved final report can settle the remaining stock.
    pub fn settles_completion_materials(&self) -> bool {
        self.event.action == super::queue_state::ApparatusQueueAction::Complete
            && self.event.payload_json.get("stage_work_report_submitted")
                .and_then(serde_json::Value::as_bool) == Some(true)
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
        self.attach_qolip_set_details(qolip_codes, set_id, None);
    }

    pub fn attach_qolip_catalog_set(&mut self, specs: &[crate::core::qolip::QolipProductSpec]) {
        let Some(first) = specs.iter().find(|spec| !spec.is_extension).or_else(|| specs.first()) else { return; };
        let codes = specs.iter().map(|spec| spec.qolip_code.clone()).collect::<Vec<_>>();
        let snapshot = serde_json::json!({"set_id": first.set_id(), "members": specs.iter().map(|spec| {
            serde_json::json!({"qolip_code": spec.qolip_code, "size": spec.size, "color": spec.color,
                "qolip_set_id": spec.set_id(), "is_extension": spec.is_extension})
        }).collect::<Vec<_>>()});
        self.attach_qolip_set_details(&codes, &first.set_id(), Some(snapshot));
    }

    fn attach_qolip_set_details(&mut self, qolip_codes: &[String], set_id: &str, snapshot: Option<serde_json::Value>) {
        let Some(mut lineage) = QolipLineage::from_codes(qolip_codes) else {
            return;
        };
        lineage.qolip_set_id = set_id.to_string();
        lineage.qolip_set_snapshot = snapshot;
        if let Some(session) = &mut self.session {
            if session.qolip_reacquisition_required() {
                self.event.payload_json["qolip_reacquired_after_freeze"] = serde_json::json!(true);
                if let Some(previous) = QolipLineage::from_payload(&session.payload_json)
                    && previous != lineage
                {
                    self.event.payload_json["qolip_replaced_on_resume"] = serde_json::json!({
                        "previous": previous, "current": lineage,
                        "previous_confirmation": session.payload_json.get("qolip_confirmation"),
                    });
                    if let Some(payload) = session.payload_json.as_object_mut() {
                        payload.remove("qolip_confirmation");
                    }
                }
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
