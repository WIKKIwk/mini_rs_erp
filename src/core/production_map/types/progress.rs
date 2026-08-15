use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::core::production_map::queue_state;

use super::{ProductionMapDefinition, ProductionOrderLogEntry, QueueActionActor};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderRunStatus {
    Active,
    Paused,
    Frozen,
    RollDetached,
    Completed,
}

impl OrderRunStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "frozen" => Some(Self::Frozen),
            "roll_detached" => Some(Self::RollDetached),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Frozen => "frozen",
            Self::RollDetached => "roll_detached",
            Self::Completed => "completed",
        }
    }

    pub fn is_open(self) -> bool {
        matches!(
            self,
            Self::Active | Self::Paused | Self::Frozen | Self::RollDetached
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderProgressBatchStatus {
    Paused,
    RollDetached,
    Completed,
    Resumed,
}

impl OrderProgressBatchStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "paused" => Some(Self::Paused),
            "roll_detached" => Some(Self::RollDetached),
            "completed" => Some(Self::Completed),
            "resumed" => Some(Self::Resumed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::RollDetached => "roll_detached",
            Self::Completed => "completed",
            Self::Resumed => "resumed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderProgressBatchWipStatus {
    Waiting,
    InUse,
    Processed,
}

impl OrderProgressBatchWipStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "waiting" => Some(Self::Waiting),
            "in_use" => Some(Self::InUse),
            "processed" => Some(Self::Processed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::InUse => "in_use",
            Self::Processed => "processed",
        }
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
    pub order_status: String,
    pub work_status: String,
    pub flow_status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stock_status: String,
    #[serde(default)]
    pub total_wip_count: usize,
    #[serde(default)]
    pub waiting_wip_count: usize,
    #[serde(default)]
    pub in_use_wip_count: usize,
    #[serde(default)]
    pub processed_wip_count: usize,
    #[serde(default)]
    pub waiting_next_stage_count: usize,
    #[serde(default)]
    pub consumed_by_next_stage_count: usize,
    #[serde(default, alias = "finished_pending_acceptance_count")]
    pub free_wip_count: usize,
    #[serde(default)]
    pub accepted_wip_count: usize,
    #[serde(default)]
    pub active_session_count: usize,
    #[serde(default)]
    pub paused_session_count: usize,
    #[serde(default)]
    pub roll_detached_session_count: usize,
    #[serde(default)]
    pub completed_queue_count: usize,
    #[serde(default)]
    pub completed_with_issue_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderRunSession {
    pub session_id: String,
    pub apparatus: String,
    pub order_id: String,
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
    pub current_apparatus_key: String,
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
        if !corrected.payload_json.is_object() {
            corrected.payload_json = serde_json::json!({});
        }
        corrected.payload_json["produced_qty"] = serde_json::json!(corrected.produced_qty);
        corrected.payload_json["uom"] = serde_json::json!(corrected.uom);
        corrected.payload_json["return_ink_kg"] = serde_json::json!(corrected.return_ink_kg);
        corrected.payload_json["lamination_print_leftover_rolls"] =
            serde_json::json!(corrected.lamination_print_leftover_rolls);
        corrected.payload_json["lamination_film_leftover_rolls"] =
            serde_json::json!(corrected.lamination_film_leftover_rolls);
        corrected.payload_json["rezka_bosma_waste"] =
            serde_json::json!(corrected.rezka_bosma_waste);
        corrected.payload_json["rezka_lamination_waste"] =
            serde_json::json!(corrected.rezka_lamination_waste);
        corrected.payload_json["rezka_edge_waste"] =
            serde_json::json!(corrected.rezka_edge_waste);
        corrected.payload_json["total_waste"] = serde_json::json!(corrected.total_waste);
        corrected.payload_json["finished_goods_kg"] =
            serde_json::json!(corrected.finished_goods_kg);
        corrected.payload_json["bobina_kg"] = serde_json::json!(corrected.bobina_kg);
        corrected.payload_json["finished_goods_meter"] =
            serde_json::json!(corrected.finished_goods_meter);
        corrected.payload_json["diameter"] = serde_json::json!(corrected.diameter);
        corrected.payload_json["description"] = serde_json::json!(corrected.description);
        corrected.payload_json["correction_revision"] = serde_json::json!(corrected.revision);
        corrected
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

/// Per-frame Rezka measurements supplied with a queue progress action.
///
/// The field names intentionally match the existing queue-action contract so a
/// mobile client can move the same measurements into a per-frame array without
/// introducing a second vocabulary.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
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
}

impl RezkaFrameProgressInput {
    pub fn to_queue_progress(
        &self,
        base: &QueueProgressInput,
        inherit_global_waste: bool,
    ) -> QueueProgressInput {
        let mut frame = base.clone();
        frame.rezka_frames.clear();
        frame.produced_qty = self.produced_qty;
        frame.gross_qty = self.gross_qty;
        frame.uom = if self.produced_qty.is_some() || self.finished_goods_meter.is_some() {
            "m".to_string()
        } else {
            base.uom.clone()
        };
        frame.rezka_bosma_waste = self
            .rezka_bosma_waste
            .or_else(|| inherit_global_waste.then_some(base.rezka_bosma_waste).flatten());
        frame.rezka_lamination_waste = self.rezka_lamination_waste.or_else(|| {
            inherit_global_waste
                .then_some(base.rezka_lamination_waste)
                .flatten()
        });
        frame.rezka_edge_waste = self
            .rezka_edge_waste
            .or_else(|| inherit_global_waste.then_some(base.rezka_edge_waste).flatten());
        frame.total_waste = self
            .total_waste
            .or_else(|| inherit_global_waste.then_some(base.total_waste).flatten());
        frame.finished_goods_kg = self.finished_goods_kg;
        frame.bobina_kg = self.bobina_kg;
        frame.finished_goods_meter = self.finished_goods_meter;
        frame.diameter = self.diameter;
        frame
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
