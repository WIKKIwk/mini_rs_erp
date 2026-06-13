use serde::{Deserialize, Serialize};

use crate::core::werka::models::DispatchRecord;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerHomeSummary {
    pub pending_count: i64,
    pub confirmed_count: i64,
    pub rejected_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomerDeliveryResponseMode {
    AcceptAll,
    AcceptPartial,
    RejectAll,
    ClaimAfterAccept,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomerDeliveryDetail {
    pub record: DispatchRecord,
    pub can_approve: bool,
    pub can_reject: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub can_partially_accept: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub can_report_claim: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomerDeliveryResponseRequest {
    pub delivery_note_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approve: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<CustomerDeliveryResponseMode>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub accepted_qty: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub returned_qty: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}
