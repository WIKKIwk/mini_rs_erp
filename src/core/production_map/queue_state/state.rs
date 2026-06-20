use serde::{Deserialize, Serialize};

use super::super::ProductionMapError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApparatusQueueOrderState {
    Pending,
    InProgress,
    Paused,
    Completed,
}

impl ApparatusQueueOrderState {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Paused => "paused",
            Self::Completed => "completed",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::InProgress | Self::Paused)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApparatusQueueAction {
    Start,
    Pause,
    Resume,
    Complete,
}

pub fn next_queue_state(
    current: ApparatusQueueOrderState,
    action: ApparatusQueueAction,
) -> Result<ApparatusQueueOrderState, ProductionMapError> {
    match action {
        ApparatusQueueAction::Start => {
            if current == ApparatusQueueOrderState::Pending {
                Ok(ApparatusQueueOrderState::InProgress)
            } else {
                Err(ProductionMapError::QueueActionNotAllowed)
            }
        }
        ApparatusQueueAction::Complete => {
            if current == ApparatusQueueOrderState::InProgress {
                Ok(ApparatusQueueOrderState::Completed)
            } else {
                Err(ProductionMapError::QueueActionNotAllowed)
            }
        }
        ApparatusQueueAction::Pause => {
            if current == ApparatusQueueOrderState::InProgress {
                Ok(ApparatusQueueOrderState::Paused)
            } else {
                Err(ProductionMapError::QueueActionNotAllowed)
            }
        }
        ApparatusQueueAction::Resume => {
            if current == ApparatusQueueOrderState::Paused {
                Ok(ApparatusQueueOrderState::InProgress)
            } else {
                Err(ProductionMapError::QueueActionNotAllowed)
            }
        }
    }
}
