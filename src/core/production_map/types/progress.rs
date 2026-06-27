use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::core::production_map::queue_state;

use super::{ProductionMapDefinition, ProductionOrderLogEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderRunStatus {
    Active,
    Paused,
    Completed,
}

impl OrderRunStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderProgressBatchStatus {
    Paused,
    Completed,
    Resumed,
}

impl OrderProgressBatchStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "resumed" => Some(Self::Resumed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paused => "paused",
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
    #[serde(default)]
    pub finished_pending_acceptance_count: usize,
    #[serde(default)]
    pub accepted_wip_count: usize,
    #[serde(default)]
    pub active_session_count: usize,
    #[serde(default)]
    pub paused_session_count: usize,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderProgressBatch {
    pub batch_id: String,
    pub session_id: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub payload_json: serde_json::Value,
}

impl OrderProgressBatch {
    pub fn refresh_status_detail(&mut self) {
        self.status_detail = OrderProgressBatchStatusDetail::from_batch(self);
    }
}

impl OrderProgressBatchStatusDetail {
    pub fn from_batch(batch: &OrderProgressBatch) -> Self {
        let work_status = match batch.status {
            OrderProgressBatchStatus::Paused => "paused",
            OrderProgressBatchStatus::Resumed => "in_progress",
            OrderProgressBatchStatus::Completed => "completed",
        }
        .to_string();
        let wip_status = batch.wip_status.as_str().to_string();
        let processed_by = batch.processed_by_apparatus.trim();
        let is_final_output = batch.action == queue_state::ApparatusQueueAction::Complete
            && batch.status == OrderProgressBatchStatus::Completed
            && batch.next_apparatus.trim().is_empty();
        let flow_status = match batch.wip_status {
            OrderProgressBatchWipStatus::Waiting if is_final_output => {
                "finished_pending_acceptance"
            }
            OrderProgressBatchWipStatus::Waiting => "waiting_next_stage",
            OrderProgressBatchWipStatus::InUse => "in_progress",
            OrderProgressBatchWipStatus::Processed
                if processed_by.to_ascii_lowercase().starts_with("warehouse:") =>
            {
                "accepted_to_stock"
            }
            OrderProgressBatchWipStatus::Processed => "consumed_by_next_stage",
        }
        .to_string();
        let stock_status = match flow_status.as_str() {
            "finished_pending_acceptance" => "pending_acceptance",
            "accepted_to_stock" => "accepted",
            _ => "",
        }
        .to_string();
        Self {
            work_status,
            wip_status,
            flow_status,
            stock_status,
        }
    }
}

impl ProductionOrderStatusDetail {
    pub fn from_order_flow(
        progress_batches: &[OrderProgressBatch],
        run_sessions: &[OrderRunSession],
        queue_states: &BTreeMap<String, BTreeMap<String, String>>,
        logs: &[ProductionOrderLogEntry],
    ) -> Self {
        let mut detail = Self::default();
        for session in run_sessions {
            match session.status {
                OrderRunStatus::Active => detail.active_session_count += 1,
                OrderRunStatus::Paused => detail.paused_session_count += 1,
                OrderRunStatus::Completed => {}
            }
        }
        for batch in progress_batches {
            let mut batch = batch.clone();
            batch.refresh_status_detail();
            detail.total_wip_count += 1;
            match batch.wip_status {
                OrderProgressBatchWipStatus::Waiting => detail.waiting_wip_count += 1,
                OrderProgressBatchWipStatus::InUse => detail.in_use_wip_count += 1,
                OrderProgressBatchWipStatus::Processed => detail.processed_wip_count += 1,
            }
            match batch.status_detail.flow_status.as_str() {
                "waiting_next_stage" => detail.waiting_next_stage_count += 1,
                "in_progress" => {}
                "consumed_by_next_stage" => detail.consumed_by_next_stage_count += 1,
                "finished_pending_acceptance" => detail.finished_pending_acceptance_count += 1,
                "accepted_to_stock" => detail.accepted_wip_count += 1,
                _ => {}
            }
        }

        let has_pending_queue = queue_states
            .values()
            .flat_map(|states| states.values())
            .any(|state| state == "pending");
        let has_in_progress_queue = queue_states
            .values()
            .flat_map(|states| states.values())
            .any(|state| state == "in_progress");
        detail.completed_queue_count = queue_states
            .values()
            .flat_map(|states| states.values())
            .filter(|state| state.as_str() == "completed")
            .count();
        detail.completed_with_issue_count = logs
            .iter()
            .filter(|entry| entry.completed_with_issue)
            .count();
        let all_wips_are_final_pending = detail.finished_pending_acceptance_count > 0
            && detail.waiting_wip_count == detail.finished_pending_acceptance_count
            && detail.in_use_wip_count == 0
            && detail.waiting_next_stage_count == 0;
        let all_final_wips_are_accepted = detail.accepted_wip_count > 0
            && detail.waiting_wip_count == 0
            && detail.in_use_wip_count == 0
            && detail.waiting_next_stage_count == 0
            && detail.finished_pending_acceptance_count == 0;

        let order_status = if detail.active_session_count > 0
            || detail.in_use_wip_count > 0
            || has_in_progress_queue
        {
            "in_progress"
        } else if all_final_wips_are_accepted {
            "accepted"
        } else if all_wips_are_final_pending {
            "finished_pending_acceptance"
        } else if detail.processed_wip_count > 0
            || detail.finished_pending_acceptance_count > 0
            || detail.consumed_by_next_stage_count > 0
        {
            "partially_completed"
        } else if detail.completed_with_issue_count > 0 {
            "completed_with_issue"
        } else if detail.completed_queue_count > 0 {
            "completed"
        } else if detail.paused_session_count > 0 {
            "paused"
        } else if detail.waiting_next_stage_count > 0 {
            "waiting_next_stage"
        } else if detail.waiting_wip_count > 0 || has_pending_queue {
            "ready"
        } else {
            "not_started"
        };
        detail.order_status = order_status.to_string();
        detail.work_status = match order_status {
            "in_progress" => "in_progress",
            "paused" => "paused",
            "accepted" | "finished_pending_acceptance" | "completed" | "completed_with_issue" => {
                "completed"
            }
            "partially_completed" => "partially_completed",
            "waiting_next_stage" | "ready" => "waiting",
            _ => "not_started",
        }
        .to_string();
        detail.flow_status = match order_status {
            "accepted" => "accepted_to_stock",
            "finished_pending_acceptance" => "finished_pending_acceptance",
            "completed_with_issue" => "completed_with_issue",
            "completed" => "completed",
            "partially_completed" => "partially_completed",
            "in_progress" => "in_progress",
            "paused" => "paused",
            "waiting_next_stage" => "waiting_next_stage",
            "ready" => "ready",
            _ => "not_started",
        }
        .to_string();
        detail.stock_status = match order_status {
            "accepted" => "accepted",
            "finished_pending_acceptance" => "pending_acceptance",
            _ => "",
        }
        .to_string();
        detail
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueueProgressInput {
    pub produced_qty: Option<f64>,
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
    pub finished_goods_meter: Option<f64>,
    pub description: String,
}
