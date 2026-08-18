use serde::{Deserialize, Serialize};

use super::progress::OrderProgressBatch;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaddonSnapshot {
    pub paddon: PaddonSummary,
    pub items: Vec<OrderProgressBatch>,
    pub available_items: Vec<OrderProgressBatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaddonCreateInput {
    pub location: String,
    pub note: String,
    pub actor_ref: String,
    pub actor_display_name: String,
}
