use serde::{Deserialize, Serialize};

use super::QueueActionActor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderControlState {
    Active,
    FreezeRequested,
    Frozen,
}

impl OrderControlState {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "freeze_requested" => Some(Self::FreezeRequested),
            "frozen" => Some(Self::Frozen),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::FreezeRequested => "freeze_requested",
            Self::Frozen => "frozen",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderFreezeRequestStatus {
    Pending,
    Frozen,
    Cancelled,
    Unfrozen,
}

impl OrderFreezeRequestStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "frozen" => Some(Self::Frozen),
            "cancelled" => Some(Self::Cancelled),
            "unfrozen" => Some(Self::Unfrozen),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Frozen => "frozen",
            Self::Cancelled => "cancelled",
            Self::Unfrozen => "unfrozen",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderFreezeRequest {
    pub request_id: String,
    pub status: OrderFreezeRequestStatus,
    pub target_session_id: String,
    pub target_apparatus: String,
    pub target_worker_role: String,
    pub target_worker_ref: String,
    pub target_worker_display_name: String,
    pub requested_at_unix: i64,
    pub transitioned_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderControlRecord {
    pub order_id: String,
    pub state: OrderControlState,
    pub actor: QueueActionActor,
    pub requested_at_unix: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen_at_unix: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeze_request: Option<OrderFreezeRequest>,
}

impl OrderControlRecord {
    pub fn active(order_id: &str) -> Self {
        Self {
            order_id: order_id.trim().to_string(),
            state: OrderControlState::Active,
            actor: QueueActionActor {
                role: String::new(),
                ref_: String::new(),
                display_name: String::new(),
            },
            requested_at_unix: 0,
            frozen_at_unix: None,
            freeze_request: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderDeleteBlocker {
    pub code: String,
    pub message: String,
}

impl OrderDeleteBlocker {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrderDeleteResult {
    pub order_id: String,
    pub deleted: bool,
}
