use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::core::production_map::queue_state;
use crate::core::returned_paint::ReturnedPaintRequest;

use super::super::store_port::RawMaterialStockTransition;
use super::progress::{OrderProgressBatch, OrderRunSession};
use super::queue::ApparatusQueueActionEvent;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionRequestNotification {
    pub event_id: String,
    pub apparatus: String,
    pub order_id: String,
    pub order_number: String,
    pub order_title: String,
    pub product_code: String,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    pub description: String,
    #[serde(default)]
    pub zero_metric_codes: Vec<String>,
    #[serde(default)]
    pub notice_kind: String,
    #[serde(default = "default_decision_required")]
    pub decision_required: bool,
    pub created_at_unix: i64,
    #[serde(skip)]
    pub returned_paint_report: Option<ReturnedPaintRequest>,
}

fn default_decision_required() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionRequestDecision {
    Approved,
    Rejected,
}

impl CompletionRequestDecision {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("approve") || value.eq_ignore_ascii_case("approved") {
            Some(Self::Approved)
        } else if value.eq_ignore_ascii_case("reject") || value.eq_ignore_ascii_case("rejected") {
            Some(Self::Rejected)
        } else {
            None
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionRequestDecisionNotification {
    pub event_id: String,
    pub request_event_id: String,
    pub decision: String,
    pub apparatus: String,
    pub order_id: String,
    pub order_number: String,
    pub order_title: String,
    pub product_code: String,
    pub worker_role: String,
    pub worker_ref: String,
    pub worker_display_name: String,
    pub decided_by_role: String,
    pub decided_by_ref: String,
    pub decided_by_display_name: String,
    pub description: String,
    pub message: String,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionOrderTransferDetails {
    pub transfer_id: String,
    pub from_apparatus: String,
    pub to_apparatus: String,
    pub reason: String,
    pub session_id: String,
    pub progress_batch_id: String,
    #[serde(default)]
    pub material_barcodes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionOrderFreezeDetails {
    pub request_id: String,
    pub status: String,
    pub target_session_id: String,
    pub target_apparatus: String,
    pub target_worker_role: String,
    pub target_worker_ref: String,
    pub target_worker_display_name: String,
    pub requested_at_unix: i64,
    pub transitioned_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionOrderLogEntry {
    pub event_id: String,
    pub apparatus: String,
    pub order_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stage_node_id: String,
    pub action: queue_state::ApparatusQueueAction,
    pub from_state: queue_state::ApparatusQueueOrderState,
    pub to_state: queue_state::ApparatusQueueOrderState,
    pub actor_role: String,
    pub actor_ref: String,
    pub actor_display_name: String,
    pub created_at_unix: i64,
    #[serde(default)]
    pub completed_with_issue: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub issue_note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer: Option<ProductionOrderTransferDetails>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeze: Option<ProductionOrderFreezeDetails>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FullyCompletedProductionOrder {
    pub order_id: String,
    pub order_number: String,
    pub title: String,
    pub product_code: String,
    pub completed_at_unix: i64,
    pub closed_by_role: String,
    pub closed_by_ref: String,
    pub closed_by_display_name: String,
    pub logs: Vec<ProductionOrderLogEntry>,
    #[serde(default)]
    pub progress_batches: Vec<OrderProgressBatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompletionRequestResult {
    pub states: BTreeMap<String, String>,
    pub completion_request: CompletionRequestNotification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompletionRequestDecisionResult {
    pub states: BTreeMap<String, String>,
    pub decision: CompletionRequestDecisionNotification,
    #[serde(skip)]
    pub raw_material_stock_warehouses: Vec<String>,
    #[serde(skip)]
    pub raw_material_stock_committed: bool,
}

#[derive(Debug, Clone)]
pub struct CompletionRequestStateResolution {
    pub apparatus: String,
    pub states: BTreeMap<String, String>,
    pub event: ApparatusQueueActionEvent,
    pub session: Option<OrderRunSession>,
    pub raw_material_stock_transitions: Vec<RawMaterialStockTransition>,
    pub returned_paint_report: Option<ReturnedPaintRequest>,
}
