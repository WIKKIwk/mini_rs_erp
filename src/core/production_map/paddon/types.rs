use serde::{Deserialize, Serialize};

use super::progress::OrderProgressBatch;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaddonReceipt {
    pub paddon: PaddonSummary,
    pub items: Vec<OrderProgressBatch>,
    pub stocks: Vec<super::FinishedGoodsStockEntry>,
    pub warehouse: String,
    pub accepted_by_ref: String,
    pub accepted_by_display_name: String,
    pub accepted_at_unix: i64,
}

#[derive(Debug, Clone)]
pub struct PaddonReceiveWrite {
    pub originals: Vec<OrderProgressBatch>,
    pub receipt: PaddonReceipt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaddonSummary {
    pub id: String,
    pub code: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub created_by_ref: String,
    #[serde(default)]
    pub created_by_display_name: String,
    pub created_at_unix: i64,
    pub updated_at_unix: i64,
    pub item_count: i64,
    #[serde(default)]
    pub locked_at_unix: Option<i64>,
    /// Product weights only; pallet tare is excluded. None means incomplete knowledge.
    #[serde(default)]
    pub total_gross_kg: Option<f64>,
    #[serde(default)]
    pub total_net_kg: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaddonSnapshot {
    pub paddon: PaddonSummary,
    pub items: Vec<OrderProgressBatch>,
    pub available_items: Vec<OrderProgressBatch>,
    #[serde(default)]
    pub free_movement_enabled: bool,
    #[serde(default)]
    pub can_manage_items: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaddonManagementSettings {
    pub free_movement_enabled: bool,
    #[serde(default)]
    pub worker_visibility_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaddonPrintConfirmation {
    pub paddon: PaddonSummary,
    pub newly_locked: bool,
    pub apparatuses: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaddonCreateInput {
    pub location: String,
    pub note: String,
    pub actor_ref: String,
    pub actor_display_name: String,
}

pub(crate) fn paddon_unlock_actor_allowed(
    locked_by_ref: &str,
    free_movement_enabled: bool,
    actor: &crate::core::production_map::QueueActionActor,
) -> bool {
    !actor.ref_.trim().is_empty()
        && (actor.role == "admin"
            || free_movement_enabled
            || (actor.role == "aparatchi" && locked_by_ref.trim() == actor.ref_.trim()))
}
