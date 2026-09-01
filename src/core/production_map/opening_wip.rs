use serde::{Deserialize, Serialize};

use super::QueueActionActor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningWipQuantityBasis {
    Measured,
    Estimated,
    Unknown,
}

impl OpeningWipQuantityBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Measured => "measured",
            Self::Estimated => "estimated",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("measured") {
            Some(Self::Measured)
        } else if value.eq_ignore_ascii_case("estimated") {
            Some(Self::Estimated)
        } else if value.eq_ignore_ascii_case("unknown") {
            Some(Self::Unknown)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningWipIntakeStatus {
    Confirmed,
    Cancelled,
}

impl OpeningWipIntakeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("confirmed") {
            Some(Self::Confirmed)
        } else if value.eq_ignore_ascii_case("cancelled") {
            Some(Self::Cancelled)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningWipBatchStatus {
    Waiting,
    InUse,
    Processed,
    Void,
}

impl OpeningWipBatchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::InUse => "in_use",
            Self::Processed => "processed",
            Self::Void => "void",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("waiting") {
            Some(Self::Waiting)
        } else if value.eq_ignore_ascii_case("in_use") {
            Some(Self::InUse)
        } else if value.eq_ignore_ascii_case("processed") {
            Some(Self::Processed)
        } else if value.eq_ignore_ascii_case("void") {
            Some(Self::Void)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningWipBatchInput {
    pub quantity_basis: OpeningWipQuantityBasis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bobina_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningWipCreateInput {
    pub idempotency_key: String,
    pub order_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub entry_apparatus: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_operation: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_apparatus: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_stage_node_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_location: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub batches: Vec<OpeningWipBatchInput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningWipIntake {
    pub intake_id: String,
    pub idempotency_key: String,
    #[serde(skip_serializing)]
    pub request_fingerprint: String,
    pub order_id: String,
    pub entry_apparatus: String,
    pub source_operation: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_apparatus: String,
    pub current_location: String,
    pub resume_apparatus: String,
    pub resume_stage_node_id: String,
    pub history_status: String,
    pub status: OpeningWipIntakeStatus,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub actor: QueueActionActor,
    pub created_at_unix: i64,
    pub updated_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningWipBatch {
    pub batch_id: String,
    pub intake_id: String,
    pub order_id: String,
    pub sequence_no: i32,
    pub qr_payload: String,
    pub quantity_basis: OpeningWipQuantityBasis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub uom: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_meter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_goods_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bobina_kg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter: Option<f64>,
    pub wip_status: OpeningWipBatchStatus,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub used_by_session_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub used_by_apparatus: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub processed_by_session_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub processed_by_apparatus: String,
    pub label_item_code: String,
    pub label_item_name: String,
    pub created_at_unix: i64,
    pub updated_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningWipRecord {
    pub intake: OpeningWipIntake,
    pub batches: Vec<OpeningWipBatch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpeningWipBatchRecord {
    pub intake: OpeningWipIntake,
    pub batch: OpeningWipBatch,
}

#[derive(Debug, Clone)]
pub struct OpeningWipCreateWrite {
    pub record: OpeningWipRecord,
}

#[derive(Debug, Clone)]
pub struct OpeningWipDeleteWrite {
    pub batch_id: String,
    pub actor: QueueActionActor,
    pub deleted_at_unix: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpeningWipQuery {
    pub order_id: String,
    pub wip_status: Option<OpeningWipBatchStatus>,
    pub limit: usize,
}
