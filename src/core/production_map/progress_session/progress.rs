use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::queue_state;

use super::{
    ProductionMapDefinition, ProductionOrderLifecycleStatus, ProductionOrderLogEntry,
    QueueActionActor,
};

include!("progress_parts/statuses.rs");
/// Parse a live apparatus reference at the progress boundary.
///
/// Progress records may retain display snapshots in their existing string
/// fields, but identity matching is only valid for canonical IDs. In
/// particular, this intentionally does not resolve a title or warehouse
/// instance to an ID.
pub fn canonical_apparatus_id(value: &str) -> Option<ApparatusId> {
    ApparatusId::new(value.trim().to_string()).ok()
}

pub fn canonical_apparatus_key(value: &str) -> String {
    let value = value.trim();
    if ApparatusId::is_valid(value) {
        value.to_string()
    } else {
        String::new()
    }
}

pub fn apparatus_ids_match(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    left == right && ApparatusId::is_valid(left)
}

/// Compare a topology stage identity. Apparatus stages use canonical
/// `ApparatusId`; non-apparatus task stages use their stable graph identity.
/// This is an exact identity comparison and never resolves display titles.
pub fn stage_ids_match(left: &str, right: &str) -> bool {
    if apparatus_ids_match(left, right) {
        return true;
    }
    let left = left.trim();
    let right = right.trim();
    is_stable_task_stage_id(left) && is_stable_task_stage_id(right) && left == right
}

/// Validated Qolip lineage carried by a progress/session payload.
///
/// The payload remains backward-compatible with the existing JSON fields, but
/// every producer/consumer crosses this typed boundary so a malformed or
/// title-derived value cannot become lineage state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct QolipLineage {
    pub(crate) qolip_code: String,
    #[serde(default)]
    pub(crate) qolip_codes: Vec<String>,
}

impl QolipLineage {
    pub(crate) fn from_codes(codes: &[String]) -> Option<Self> {
        let mut normalized = Vec::new();
        for code in codes {
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
        normalized.first().cloned().map(|qolip_code| Self {
            qolip_code,
            qolip_codes: normalized,
        })
    }

    pub(crate) fn from_payload(payload: &serde_json::Value) -> Option<Self> {
        let primary = payload
            .get("qolip_code")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|code| !code.is_empty())
            .map(str::to_string);
        let mut codes = Vec::new();
        if let Some(values) = payload.get("qolip_codes") {
            for value in values.as_array()? {
                let code = value.as_str()?.trim();
                if code.is_empty() {
                    return None;
                }
                codes.push(code.to_string());
            }
        }
        if let Some(primary) = primary {
            codes.insert(0, primary);
        }
        Self::from_codes(&codes)
    }

    pub(crate) fn write_to_payload(&self, payload: &mut serde_json::Value) {
        if !payload.is_object() {
            *payload = serde_json::json!({});
        }
        payload["qolip_code"] = serde_json::json!(self.qolip_code);
        payload["qolip_codes"] = serde_json::json!(self.qolip_codes);
    }
}

pub(crate) fn qolip_lineage_from_batch(batch: &OrderProgressBatch) -> Option<QolipLineage> {
    QolipLineage::from_payload(&batch.payload_json)
}

fn is_stable_task_stage_id(value: &str) -> bool {
    let Some(task_id) = value.strip_prefix("task:") else {
        return false;
    };
    !task_id.is_empty()
        && task_id.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_' | ':' | '.')
        })
}


/// One upstream WIP consumed by a production run session.
///
/// `sequence_no` preserves splice order. Quantity contribution is
/// intentionally absent because the worker flow does not measure it at the
/// merge boundary; inventing an allocation would corrupt accounting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderRunInputLink {
    pub input_batch_id: String,
    pub input_qr_payload: String,
    pub source_apparatus: String,
    pub source_kind: OrderRunInputSourceKind,
    pub stage_node_id: String,
    pub sequence_no: u32,
    pub status: OrderRunInputStatus,
    pub linked_at_unix: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed_at_unix: Option<i64>,
}

impl OrderRunInputLink {
    pub(crate) fn is_valid(&self) -> bool {
        !self.input_batch_id.trim().is_empty()
            && self.sequence_no > 0
            && match self.status {
                OrderRunInputStatus::InUse => self.processed_at_unix.is_none(),
                OrderRunInputStatus::Processed => self.processed_at_unix.is_some(),
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RezkaPartialRollStatus {
    Active,
}

impl RezkaPartialRollStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
        }
    }
}

/// The unfinished physical output roll currently mounted in one Rezka frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RezkaActivePartialRoll {
    pub slot_index: u32,
    pub generation: u32,
    pub contained_kadr_count: u32,
    pub status: RezkaPartialRollStatus,
    #[serde(default)]
    pub source_input_batch_ids: Vec<String>,
    pub started_at_unix: i64,
    pub updated_at_unix: i64,
}

impl RezkaActivePartialRoll {
    pub(crate) fn is_valid(&self) -> bool {
        if self.slot_index == 0 || self.generation == 0 || self.contained_kadr_count == 0 {
            return false;
        }
        let mut unique = std::collections::BTreeSet::new();
        self.source_input_batch_ids
            .iter()
            .all(|batch_id| !batch_id.trim().is_empty() && unique.insert(batch_id.trim()))
    }
}

/// Auditable many-to-one lineage for one completed output batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressBatchInputLink {
    pub input_batch_id: String,
    pub input_qr_payload: String,
    pub source_apparatus: String,
    pub source_kind: OrderRunInputSourceKind,
    pub sequence_no: u32,
}

impl ProgressBatchInputLink {
    pub(crate) fn is_valid(&self) -> bool {
        !self.input_batch_id.trim().is_empty() && self.sequence_no > 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WipProgressBatchQuery {
    pub apparatus: String,
    pub next_apparatus: String,
    pub current_location: String,
    pub status: Option<OrderProgressBatchWipStatus>,
    pub include_processed: bool,
    pub order_id: String,
    pub limit: usize,
}

impl WipProgressBatchQuery {
    pub fn new(
        apparatus: &str,
        next_apparatus: &str,
        current_location: &str,
        status: Option<OrderProgressBatchWipStatus>,
        include_processed: bool,
        order_id: &str,
        limit: usize,
    ) -> Self {
        Self {
            apparatus: apparatus.trim().to_string(),
            next_apparatus: next_apparatus.trim().to_string(),
            current_location: current_location.trim().to_string(),
            status,
            include_processed,
            order_id: order_id.trim().to_string(),
            limit,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderProgressBatchStatusDetail {
    pub work_status: String,
    pub wip_status: String,
    pub flow_status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stock_status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionOrderStatusDetail {
    #[serde(default)]
    pub lifecycle_status: ProductionOrderLifecycleStatus,
    pub order_status: String,
    pub work_status: String,
    pub flow_status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stock_status: String,
    #[serde(default)]
    pub completed_with_issue_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderRunSession {
    pub session_id: String,
    pub apparatus: String,
    pub order_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stage_node_id: String,
    pub status: OrderRunStatus,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    pub started_at_unix: i64,
    pub updated_at_unix: i64,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderProgressEvent {
    pub event_id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub batch_id: String,
    pub apparatus: String,
    pub order_id: String,
    pub action: queue_state::ApparatusQueueAction,
    pub produced_qty: f64,
    pub uom: String,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub qr_payload: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_ink_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lamination_print_leftover_rolls: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lamination_film_leftover_rolls: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_bosma_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_lamination_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_edge_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "babina_kg")]
    pub bobina_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaminatsiyaAstatkaReport {
    pub report_id: String,
    pub order_id: String,
    pub apparatus: String,
    pub from_at_unix: i64,
    pub to_at_unix: i64,
    pub lamination_print_leftover_rolls: f64,
    pub lamination_film_leftover_rolls: f64,
    pub total_waste: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "babina_kg")]
    pub bobina_kg: Option<f64>,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RezkaAstatkaReport {
    pub report_id: String,
    pub order_id: String,
    pub apparatus: String,
    pub from_at_unix: i64,
    pub to_at_unix: i64,
    pub total_waste: f64,
    pub rezka_bosma_waste: f64,
    pub rezka_lamination_waste: f64,
    pub rezka_edge_waste: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "babina_kg")]
    pub bobina_kg: Option<f64>,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderProgressBatch {
    pub batch_id: String,
    #[serde(default = "default_progress_batch_revision")]
    pub revision: u64,
    pub session_id: String,
    pub started_at_unix: i64,
    pub completed_at_unix: i64,
    pub apparatus: String,
    pub order_id: String,
    pub action: queue_state::ApparatusQueueAction,
    pub status: OrderProgressBatchStatus,
    pub produced_qty: f64,
    pub uom: String,
    pub qr_payload: String,
    pub label_item_code: String,
    pub label_item_name: String,
    pub executor_name: String,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    pub wip_status: OrderProgressBatchWipStatus,
    #[serde(default)]
    pub status_detail: OrderProgressBatchStatusDetail,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_apparatus: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_location: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub next_apparatus: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub parent_batch_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub used_by_session_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub used_by_apparatus: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub processed_by_session_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub processed_by_apparatus: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_ink_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lamination_print_leftover_rolls: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lamination_film_leftover_rolls: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_bosma_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_lamination_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_edge_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "babina_kg")]
    pub bobina_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub payload_json: serde_json::Value,
}

const fn default_progress_batch_revision() -> u64 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressBatchCorrectionInput {
    pub batch_id: String,
    pub expected_revision: u64,
    pub produced_qty: f64,
    pub uom: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_ink_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lamination_print_leftover_rolls: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lamination_film_leftover_rolls: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_bosma_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_lamination_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rezka_edge_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_waste: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "babina_kg")]
    pub bobina_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressBatchCorrectionRecord {
    pub batch_id: String,
    pub previous_revision: u64,
    pub new_revision: u64,
    pub reason: String,
    pub actor: QueueActionActor,
    pub old_values: serde_json::Value,
    pub new_values: serde_json::Value,
    pub created_at_unix: i64,
}

impl OrderProgressBatch {
    pub fn corrected(&self, input: &ProgressBatchCorrectionInput) -> Self {
        let mut corrected = self.clone();
        corrected.revision = self.revision.saturating_add(1);
        corrected.produced_qty = input.produced_qty;
        corrected.uom = input.uom.trim().to_string();
        corrected.return_ink_kg = input.return_ink_kg;
        corrected.lamination_print_leftover_rolls = input.lamination_print_leftover_rolls;
        corrected.lamination_film_leftover_rolls = input.lamination_film_leftover_rolls;
        corrected.rezka_bosma_waste = input.rezka_bosma_waste;
        corrected.rezka_lamination_waste = input.rezka_lamination_waste;
        corrected.rezka_edge_waste = input.rezka_edge_waste;
        corrected.total_waste = input.total_waste;
        corrected.finished_goods_kg = input.finished_goods_kg;
        corrected.bobina_kg = input.bobina_kg;
        corrected.finished_goods_meter = input.finished_goods_meter;
        corrected.diameter = input.diameter;
        corrected.description = input.description.trim().to_string();
        corrected.sync_correction_payload();
        corrected
    }

    pub(crate) fn correction_values(&self) -> serde_json::Value {
        serde_json::json!({
            "produced_qty": self.produced_qty,
            "uom": self.uom,
            "return_ink_kg": self.return_ink_kg,
            "lamination_print_leftover_rolls": self.lamination_print_leftover_rolls,
            "lamination_film_leftover_rolls": self.lamination_film_leftover_rolls,
            "rezka_bosma_waste": self.rezka_bosma_waste,
            "rezka_lamination_waste": self.rezka_lamination_waste,
            "rezka_edge_waste": self.rezka_edge_waste,
            "total_waste": self.total_waste,
            "finished_goods_kg": self.finished_goods_kg,
            "bobina_kg": self.bobina_kg,
            "finished_goods_meter": self.finished_goods_meter,
            "diameter": self.diameter,
            "description": self.description,
        })
    }

    fn sync_correction_payload(&mut self) {
        if !self.payload_json.is_object() {
            self.payload_json = serde_json::json!({});
        }
        let fields = self.correction_values();
        self.payload_json
            .as_object_mut()
            .expect("progress batch payload")
            .extend(fields.as_object().expect("correction values").clone());
        self.payload_json["correction_revision"] = serde_json::json!(self.revision);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionQrOpenedBy {
    pub actor_role: String,
    pub actor_ref: String,
    pub actor_display_name: String,
    pub opened_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionQrReport {
    pub scanned_batch: OrderProgressBatch,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_batch: Option<OrderProgressBatch>,
    pub is_stale: bool,
    pub stale_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<ProductionMapDefinition>,
    pub order_status: ProductionOrderStatusDetail,
    pub queue_states: BTreeMap<String, BTreeMap<String, String>>,
    pub logs: Vec<ProductionOrderLogEntry>,
    pub corrections: Vec<ProgressBatchCorrectionRecord>,
    pub progress_batches: Vec<OrderProgressBatch>,
    pub run_sessions: Vec<OrderRunSession>,
    pub active_sessions: Vec<OrderRunSession>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opened_by: Option<ProductionQrOpenedBy>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinishedGoodsStockEntry {
    pub id: String,
    pub warehouse: String,
    pub order_id: String,
    pub item_code: String,
    pub item_name: String,
    pub qty: f64,
    pub uom: String,
    pub status: String,
    pub barcode: String,
    pub source_progress_batch_id: String,
    pub accepted_by_role: String,
    pub accepted_by_ref: String,
    pub accepted_by_display_name: String,
    pub accepted_at_unix: i64,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinishedGoodsReceipt {
    pub batch: OrderProgressBatch,
    pub stock: FinishedGoodsStockEntry,
    pub order_status: ProductionOrderStatusDetail,
}

#[cfg(test)]
mod apparatus_identity_tests {
    use super::{
        apparatus_ids_match, canonical_apparatus_id, canonical_apparatus_key, stage_ids_match,
    };

    #[test]
    fn progress_identity_requires_canonical_ids_and_ignores_display_titles() {
        assert!(apparatus_ids_match(
            "apparatus:catalog:press-001",
            "apparatus:catalog:press-001"
        ));
        assert!(!apparatus_ids_match(
            "apparatus:catalog:press-001",
            "apparatus:catalog:press-002"
        ));
        assert!(!apparatus_ids_match(
            "apparatus:press-001",
            "apparatus:catalog:press-001"
        ));
        assert!(!apparatus_ids_match(
            "8 ta rangli pechat",
            "apparatus:catalog:press-001"
        ));
        assert!(!apparatus_ids_match(
            "task:lamination-1",
            "task:lamination-1"
        ));
        assert!(stage_ids_match("task:lamination-1", "task:lamination-1"));
        assert!(!stage_ids_match("task:", "task:"));
        assert!(!stage_ids_match("Laminatsiya", "laminatsiya"));
        assert!(!stage_ids_match("laminatsiya", "laminatsiya"));
    }

    #[test]
    fn progress_key_preserves_id_across_display_rename() {
        let id = canonical_apparatus_id("apparatus:catalog:press-001").unwrap();
        assert_eq!(
            canonical_apparatus_key(id.as_str()),
            "apparatus:catalog:press-001"
        );
        assert_eq!(canonical_apparatus_key("8 ta rangli pechat"), "");
    }

    #[test]
    fn progress_qr_remains_owned_by_the_batch_identity() {
        let batch_id = "progress-batch:123:apparatus:catalog:press-001:order-7";
        let renamed_display_batch_id = batch_id;
        assert_eq!(
            crate::core::production_map::progress_qr_payload(batch_id),
            crate::core::production_map::progress_qr_payload(renamed_display_batch_id)
        );
        assert_ne!(
            crate::core::production_map::progress_qr_payload(batch_id),
            crate::core::production_map::progress_qr_payload(
                "progress-batch:123:apparatus:catalog:press-002:order-7"
            )
        );
    }
}

/// Per-frame Rezka measurements supplied with a queue progress action.
///
/// The field names intentionally match the existing queue-action contract so a
/// mobile client can move the same measurements into a per-frame array without
/// introducing a second vocabulary.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RezkaFrameProgressInput {
    #[serde(default)]
    pub produced_qty: Option<f64>,
    #[serde(default)]
    pub gross_qty: Option<f64>,
    #[serde(default)]
    pub finished_goods_kg: Option<f64>,
    #[serde(default)]
    pub finished_goods_meter: Option<f64>,
    #[serde(default)]
    pub diameter: Option<f64>,
    #[serde(default)]
    pub bobina_kg: Option<f64>,
    #[serde(default)]
    pub rezka_bosma_waste: Option<f64>,
    #[serde(default)]
    pub rezka_lamination_waste: Option<f64>,
    #[serde(default)]
    pub rezka_edge_waste: Option<f64>,
    #[serde(default)]
    pub total_waste: Option<f64>,
    /// A frame may be completed as an issue without producing a QR/WIP
    /// output. This is accepted by Rezka roll progress actions.
    #[serde(default)]
    pub issue_note: String,
}

impl RezkaFrameProgressInput {
    pub fn to_queue_progress(
        &self,
        base: &QueueProgressInput,
        inherit_global_waste: bool,
    ) -> QueueProgressInput {
        QueueProgressInput {
            freeze_request_id: base.freeze_request_id.clone(),
            freeze_with_issue: base.freeze_with_issue,
            rezka_frames: Vec::new(),
            rezka_record_frame_index: None,
            rezka_output_cycle: String::new(),
            produced_qty: self.produced_qty,
            gross_qty: self.gross_qty,
            uom: if self.produced_qty.is_some() || self.finished_goods_meter.is_some() {
                "m".to_string()
            } else {
                base.uom.clone()
            },
            progress_batch_id: base.progress_batch_id.clone(),
            qr_payload: base.qr_payload.clone(),
            return_ink_kg: base.return_ink_kg,
            lamination_print_leftover_rolls: base.lamination_print_leftover_rolls,
            lamination_film_leftover_rolls: base.lamination_film_leftover_rolls,
            rezka_bosma_waste: self.rezka_bosma_waste.or_else(|| {
                inherit_global_waste
                    .then_some(base.rezka_bosma_waste)
                    .flatten()
            }),
            rezka_lamination_waste: self.rezka_lamination_waste.or_else(|| {
                inherit_global_waste
                    .then_some(base.rezka_lamination_waste)
                    .flatten()
            }),
            rezka_edge_waste: self.rezka_edge_waste.or_else(|| {
                inherit_global_waste
                    .then_some(base.rezka_edge_waste)
                    .flatten()
            }),
            total_waste: self
                .total_waste
                .or_else(|| inherit_global_waste.then_some(base.total_waste).flatten()),
            finished_goods_kg: self.finished_goods_kg,
            bobina_kg: self.bobina_kg,
            finished_goods_meter: self.finished_goods_meter,
            diameter: self.diameter,
            description: base.description.clone(),
            returned_paint_report_attached: base.returned_paint_report_attached,
            force_full_completion_metrics: base.force_full_completion_metrics,
            allow_partial_station_completion: base.allow_partial_station_completion,
            worker_handoff: base.worker_handoff,
            remove_roll_from_apparatus: base.remove_roll_from_apparatus,
        }
    }

    pub fn has_explicit_waste(&self) -> bool {
        self.rezka_bosma_waste.is_some()
            || self.rezka_lamination_waste.is_some()
            || self.rezka_edge_waste.is_some()
            || self.total_waste.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueueProgressInput {
    pub freeze_request_id: String,
    /// Backward-compatible marker for the legacy pause-plus-issue request.
    /// The queue action is canonicalized to `Freeze` before persistence.
    pub freeze_with_issue: bool,
    pub rezka_frames: Vec<RezkaFrameProgressInput>,
    /// One-based output slot. Records a single roll without closing the output cycle.
    pub rezka_record_frame_index: Option<usize>,
    /// Server-issued cycle identity prevents stale dialogs from creating new rolls.
    pub rezka_output_cycle: String,
    pub produced_qty: Option<f64>,
    pub gross_qty: Option<f64>,
    pub uom: String,
    pub progress_batch_id: String,
    pub qr_payload: String,
    pub return_ink_kg: Option<f64>,
    pub lamination_print_leftover_rolls: Option<f64>,
    pub lamination_film_leftover_rolls: Option<f64>,
    pub rezka_bosma_waste: Option<f64>,
    pub rezka_lamination_waste: Option<f64>,
    pub rezka_edge_waste: Option<f64>,
    pub total_waste: Option<f64>,
    pub finished_goods_kg: Option<f64>,
    pub bobina_kg: Option<f64>,
    pub finished_goods_meter: Option<f64>,
    pub diameter: Option<f64>,
    pub description: String,
    pub returned_paint_report_attached: bool,
    /// A worker may finish the currently available Laminatsiya or Rezka WIP
    /// while the upstream stage is still producing more WIPs. In that case
    /// only the finished-goods quantities are reported. This flag is computed
    /// by the queue service; clients can force the full accounting form when
    /// they intentionally leave an order for another order.
    pub force_full_completion_metrics: bool,
    pub allow_partial_station_completion: bool,
    /// Laminatsiya worker is leaving the order while the current roll remains
    /// in the apparatus. This is a handoff, not a production pause with a
    /// finished WIP output.
    pub worker_handoff: bool,
    /// The worker is removing the unfinished roll from the apparatus after a
    /// previous worker handed the order off. The roll remains unfinished and
    /// is put back into waiting WIP.
    pub remove_roll_from_apparatus: bool,
}

impl QueueProgressInput {
    pub(crate) fn has_reported_output(&self) -> bool {
        !self.rezka_frames.is_empty()
            || self.produced_qty.is_some()
            || self.gross_qty.is_some()
            || self.return_ink_kg.is_some()
            || self.lamination_print_leftover_rolls.is_some()
            || self.lamination_film_leftover_rolls.is_some()
            || self.rezka_bosma_waste.is_some()
            || self.rezka_lamination_waste.is_some()
            || self.rezka_edge_waste.is_some()
            || self.total_waste.is_some()
            || self.finished_goods_kg.is_some()
            || self.bobina_kg.is_some()
            || self.finished_goods_meter.is_some()
            || self.diameter.is_some()
    }

    pub(crate) fn has_rezka_quantity_metrics(&self) -> bool {
        let is_positive =
            |value: Option<f64>| value.is_some_and(|value| value.is_finite() && value > 0.0);
        is_positive(self.produced_qty.or(self.finished_goods_meter))
            && is_positive(self.gross_qty.or(self.finished_goods_kg))
            && is_positive(self.diameter)
    }

    pub(crate) fn has_complete_freeze_safe_stop_output(&self, is_rezka: bool) -> bool {
        if is_rezka {
            return !self.rezka_frames.is_empty()
                || (self.has_rezka_quantity_metrics() && self.bobina_kg.is_some());
        }
        self.produced_qty.or(self.finished_goods_meter).is_some()
            && self.gross_qty.or(self.finished_goods_kg).is_some()
            && self.bobina_kg.is_some()
    }
}

pub(crate) const INPUT_LINEAGE_PAYLOAD_FIELD: &str = "input_lineage";
pub(crate) const REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD: &str =
    "rezka_active_partial_rolls";
pub(crate) const SOURCE_INPUT_LINKS_PAYLOAD_FIELD: &str = "source_input_links";

pub(crate) fn order_run_input_links_from_payload(
    payload: &serde_json::Value,
) -> Result<Vec<OrderRunInputLink>, ()> {
    let Some(value) = payload.get(INPUT_LINEAGE_PAYLOAD_FIELD) else {
        return Ok(Vec::new());
    };
    let links: Vec<OrderRunInputLink> = serde_json::from_value(value.clone()).map_err(|_| ())?;
    let mut batch_ids = std::collections::BTreeSet::new();
    let mut sequence_numbers = std::collections::BTreeSet::new();
    let mut active_count = 0usize;
    for link in &links {
        if !link.is_valid()
            || !batch_ids.insert(link.input_batch_id.trim())
            || !sequence_numbers.insert(link.sequence_no)
        {
            return Err(());
        }
        if link.status == OrderRunInputStatus::InUse {
            active_count += 1;
        }
    }
    if active_count > 1 {
        return Err(());
    }
    Ok(links)
}

pub(crate) fn write_order_run_input_links(
    payload: &mut serde_json::Value,
    links: &[OrderRunInputLink],
) {
    ensure_payload_object(payload);
    payload[INPUT_LINEAGE_PAYLOAD_FIELD] = serde_json::json!(links);
}

pub(crate) fn rezka_active_partial_rolls_from_payload(
    payload: &serde_json::Value,
) -> Result<Vec<RezkaActivePartialRoll>, ()> {
    let Some(value) = payload.get(REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD) else {
        return Ok(Vec::new());
    };
    let rolls: Vec<RezkaActivePartialRoll> =
        serde_json::from_value(value.clone()).map_err(|_| ())?;
    let mut slots = std::collections::BTreeSet::new();
    for roll in &rolls {
        if !roll.is_valid() || !slots.insert(roll.slot_index) {
            return Err(());
        }
    }
    Ok(rolls)
}

pub(crate) fn write_rezka_active_partial_rolls(
    payload: &mut serde_json::Value,
    rolls: &[RezkaActivePartialRoll],
) {
    ensure_payload_object(payload);
    payload[REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD] = serde_json::json!(rolls);
}

pub(crate) fn progress_batch_input_links_from_payload(
    payload: &serde_json::Value,
) -> Result<Vec<ProgressBatchInputLink>, ()> {
    let Some(value) = payload.get(SOURCE_INPUT_LINKS_PAYLOAD_FIELD) else {
        return Ok(Vec::new());
    };
    let links: Vec<ProgressBatchInputLink> =
        serde_json::from_value(value.clone()).map_err(|_| ())?;
    let mut batch_ids = std::collections::BTreeSet::new();
    let mut sequence_numbers = std::collections::BTreeSet::new();
    for link in &links {
        if !link.is_valid()
            || !batch_ids.insert(link.input_batch_id.trim())
            || !sequence_numbers.insert(link.sequence_no)
        {
            return Err(());
        }
    }
    Ok(links)
}

pub(crate) fn rezka_merge_state_is_consistent(
    input_links: &[OrderRunInputLink],
    active_rolls: &[RezkaActivePartialRoll],
) -> bool {
    let lineage_batch_ids = input_links
        .iter()
        .map(|link| link.input_batch_id.trim())
        .collect::<std::collections::BTreeSet<_>>();
    let active_input_batch_id = input_links
        .iter()
        .find(|link| link.status == OrderRunInputStatus::InUse)
        .map(|link| link.input_batch_id.trim());

    active_rolls.iter().all(|roll| {
        let sources_exist_in_lineage = roll
            .source_input_batch_ids
            .iter()
            .all(|batch_id| lineage_batch_ids.contains(batch_id.trim()));
        let active_source_is_present = match active_input_batch_id {
            Some(active) => roll
                .source_input_batch_ids
                .iter()
                .any(|source| source.trim() == active),
            None => roll.source_input_batch_ids.is_empty(),
        };
        sources_exist_in_lineage && active_source_is_present
    })
}

pub(crate) fn write_progress_batch_input_links(
    payload: &mut serde_json::Value,
    links: &[ProgressBatchInputLink],
) {
    ensure_payload_object(payload);
    payload[SOURCE_INPUT_LINKS_PAYLOAD_FIELD] = serde_json::json!(links);
}

fn ensure_payload_object(payload: &mut serde_json::Value) {
    if !payload.is_object() {
        *payload = serde_json::json!({});
    }
}

#[cfg(test)]
mod merge_lineage_payload_tests {
    use super::*;

    fn input_link(batch_id: &str, sequence_no: u32, status: OrderRunInputStatus) -> OrderRunInputLink {
        OrderRunInputLink {
            input_batch_id: batch_id.to_string(),
            input_qr_payload: format!("qr:{batch_id}"),
            source_apparatus: "apparatus:catalog:print-001".to_string(),
            source_kind: OrderRunInputSourceKind::ProgressBatch,
            stage_node_id: "rezka".to_string(),
            sequence_no,
            status,
            linked_at_unix: 10,
            processed_at_unix: (status == OrderRunInputStatus::Processed).then_some(20),
        }
    }

    #[test]
    fn lineage_round_trip_preserves_splice_order_and_active_roll_sources() {
        let links = vec![
            input_link("wip-a", 1, OrderRunInputStatus::Processed),
            input_link("wip-b", 2, OrderRunInputStatus::InUse),
        ];
        let rolls = vec![RezkaActivePartialRoll {
            slot_index: 1,
            generation: 1,
            contained_kadr_count: 2,
            status: RezkaPartialRollStatus::Active,
            source_input_batch_ids: vec!["wip-a".to_string(), "wip-b".to_string()],
            started_at_unix: 10,
            updated_at_unix: 20,
        }];
        let mut payload = serde_json::json!({});
        write_order_run_input_links(&mut payload, &links);
        write_rezka_active_partial_rolls(&mut payload, &rolls);

        assert_eq!(order_run_input_links_from_payload(&payload), Ok(links));
        assert_eq!(rezka_active_partial_rolls_from_payload(&payload), Ok(rolls));
    }

    #[test]
    fn lineage_rejects_two_active_inputs_and_duplicate_roll_sources() {
        let mut payload = serde_json::json!({});
        write_order_run_input_links(
            &mut payload,
            &[
                input_link("wip-a", 1, OrderRunInputStatus::InUse),
                input_link("wip-b", 2, OrderRunInputStatus::InUse),
            ],
        );
        assert!(order_run_input_links_from_payload(&payload).is_err());

        payload = serde_json::json!({});
        payload[REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD] = serde_json::json!([{
                "slot_index": 1,
                "generation": 1,
                "contained_kadr_count": 1,
                "status": "active",
                "source_input_batch_ids": ["wip-a", "wip-a"],
                "started_at_unix": 10,
                "updated_at_unix": 20,
            }]);
        assert!(rezka_active_partial_rolls_from_payload(&payload).is_err());
    }

    #[test]
    fn output_lineage_rejects_duplicate_sequences() {
        let mut payload = serde_json::json!({});
        write_progress_batch_input_links(
            &mut payload,
            &[
                ProgressBatchInputLink {
                    input_batch_id: "wip-a".to_string(),
                    input_qr_payload: "qr:wip-a".to_string(),
                    source_apparatus: "apparatus:catalog:print-001".to_string(),
                    source_kind: OrderRunInputSourceKind::ProgressBatch,
                    sequence_no: 1,
                },
                ProgressBatchInputLink {
                    input_batch_id: "wip-b".to_string(),
                    input_qr_payload: "qr:wip-b".to_string(),
                    source_apparatus: "apparatus:catalog:print-001".to_string(),
                    source_kind: OrderRunInputSourceKind::ProgressBatch,
                    sequence_no: 1,
                },
            ],
        );

        assert!(progress_batch_input_links_from_payload(&payload).is_err());
    }

    #[test]
    fn active_roll_sources_must_exist_in_lineage_and_include_current_input() {
        let links = vec![
            input_link("wip-a", 1, OrderRunInputStatus::Processed),
            input_link("wip-b", 2, OrderRunInputStatus::InUse),
        ];
        let valid_roll = RezkaActivePartialRoll {
            slot_index: 1,
            generation: 1,
            contained_kadr_count: 1,
            status: RezkaPartialRollStatus::Active,
            source_input_batch_ids: vec!["wip-a".to_string(), "wip-b".to_string()],
            started_at_unix: 10,
            updated_at_unix: 20,
        };
        assert!(rezka_merge_state_is_consistent(
            &links,
            std::slice::from_ref(&valid_roll)
        ));

        let mut missing_current = valid_roll.clone();
        missing_current.source_input_batch_ids = vec!["wip-a".to_string()];
        assert!(!rezka_merge_state_is_consistent(
            &links,
            &[missing_current]
        ));

        let mut unknown_source = valid_roll;
        unknown_source.source_input_batch_ids.push("wip-c".to_string());
        assert!(!rezka_merge_state_is_consistent(
            &links,
            &[unknown_source]
        ));
    }
}
