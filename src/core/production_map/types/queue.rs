use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::core::production_map::queue_state;

use super::definition::{ProductionMapDefinition, ProductionMapSaved};
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

/// Backend-owned presentation contract for an order at one apparatus.
///
/// The mobile client may render these actions, but it must not derive them
/// from queue state, production-map topology, or WIP records. Every action is
/// still revalidated by the queue action command before it is committed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApparatusQueueOrderActionControl {
    pub state: queue_state::ApparatusQueueOrderState,
    #[serde(default)]
    pub allowed_actions: Vec<queue_state::ApparatusQueueAction>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub previous_stage: String,
    #[serde(default)]
    pub previous_stage_ready: bool,
    #[serde(default)]
    pub complete_requires_full_report: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueActionActor {
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ref_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_name: String,
}

impl Default for QueueActionActor {
    fn default() -> Self {
        Self {
            role: String::new(),
            ref_: String::new(),
            display_name: String::new(),
        }
    }
}

/// Durable receipt of an emergency apparatus transfer. The full post-transfer
/// snapshot is kept in the receipt so an idempotent retry can return exactly
/// the same result without guessing from mutable queue state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionMapApparatusTransferRecord {
    pub transfer_id: String,
    pub idempotency_key: String,
    pub order_id: String,
    pub from_apparatus: String,
    pub to_apparatus: String,
    pub reason: String,
    pub actor: QueueActionActor,
    pub session_id: String,
    pub progress_batch_id: String,
    #[serde(default)]
    pub material_barcodes: Vec<String>,
    pub map: ProductionMapDefinition,
    pub session: OrderRunSession,
    pub progress_batch: OrderProgressBatch,
    /// Parent WIP records whose next-apparatus pointer changed with the
    /// transfer. Keeping these in the receipt makes replay/audit able to
    /// reconstruct the complete lineage change, not only the paused batch.
    #[serde(default)]
    pub progress_batch_updates: Vec<OrderProgressBatch>,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProductionMapApparatusTransferResult {
    pub transfer: ProductionMapApparatusTransferRecord,
    pub saved: ProductionMapSaved,
    pub order_status: ProductionOrderStatusDetail,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletedQueueOrderStatus {
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedQueueOrder {
    pub apparatus: String,
    pub order_id: String,
    pub completed_at_unix: i64,
    pub status: CompletedQueueOrderStatus,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub progress_batches: Vec<OrderProgressBatch>,
    #[serde(skip)]
    pub raw_material_stock_warehouses: Vec<String>,
    #[serde(skip)]
    pub qolip_checkout_committed: bool,
}
