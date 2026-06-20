use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::core::production_map::queue_state;

use super::progress::OrderRunSession;
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
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionRequestDecision {
    Approved,
    Rejected,
}

impl CompletionRequestDecision {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "approve" | "approved" => Some(Self::Approved),
            "reject" | "rejected" => Some(Self::Rejected),
            _ => None,
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
pub struct ProductionOrderLogEntry {
    pub event_id: String,
    pub apparatus: String,
    pub order_id: String,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone)]
pub struct CompletionRequestStateResolution {
    pub apparatus: String,
    pub states: BTreeMap<String, String>,
    pub event: ApparatusQueueActionEvent,
    pub session: Option<OrderRunSession>,
}
