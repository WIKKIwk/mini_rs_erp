use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::core::production_map::queue_state;

use super::progress::{
    OrderProgressBatch, OrderProgressEvent, OrderRunSession, ProductionOrderStatusDetail,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApparatusQueuePolicy {
    StrictSequence,
    FreePick,
}

impl ApparatusQueuePolicy {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "strict_sequence" => Some(Self::StrictSequence),
            "free_pick" => Some(Self::FreePick),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::StrictSequence => "strict_sequence",
            Self::FreePick => "free_pick",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusQueuePolicyRecord {
    pub apparatus: String,
    pub policy: ApparatusQueuePolicy,
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueActionActor {
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ref_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusQueueActionEvent {
    pub event_id: String,
    pub apparatus: String,
    pub order_id: String,
    pub action: queue_state::ApparatusQueueAction,
    pub from_state: queue_state::ApparatusQueueOrderState,
    pub to_state: queue_state::ApparatusQueueOrderState,
    pub policy: ApparatusQueuePolicy,
    pub actor: QueueActionActor,
    #[serde(default)]
    pub assigned_apparatus: Vec<String>,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedQueueOrder {
    pub apparatus: String,
    pub order_id: String,
    pub completed_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApparatusQueueActionResult {
    pub states: BTreeMap<String, String>,
    pub order_status: ProductionOrderStatusDetail,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<OrderRunSession>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_event: Option<OrderProgressEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_batch: Option<OrderProgressBatch>,
    #[serde(skip)]
    pub raw_material_stock_warehouses: Vec<String>,
    #[serde(skip)]
    pub qolip_checkout_committed: bool,
}
