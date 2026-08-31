use serde::{Deserialize, Serialize};

use super::super::ProductionMapError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApparatusQueueOrderState {
    Pending,
    InProgress,
    Paused,
    Frozen,
    Completed,
}

impl ApparatusQueueOrderState {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "paused" => Some(Self::Paused),
            "frozen" => Some(Self::Frozen),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Paused => "paused",
            Self::Frozen => "frozen",
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
    Freeze,
    DetachRoll,
    Resume,
    Merge,
    RollComplete,
    Complete,
}

impl ApparatusQueueAction {
    pub const fn records_progress_output(self) -> bool {
        matches!(
            self,
            Self::Pause | Self::DetachRoll | Self::RollComplete | Self::Complete
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct QueueStateTransition {
    from: ApparatusQueueOrderState,
    action: ApparatusQueueAction,
    to: ApparatusQueueOrderState,
}

const QUEUE_STATE_TRANSITIONS: &[QueueStateTransition] = &[
    QueueStateTransition {
        from: ApparatusQueueOrderState::Pending,
        action: ApparatusQueueAction::Start,
        to: ApparatusQueueOrderState::InProgress,
    },
    QueueStateTransition {
        from: ApparatusQueueOrderState::InProgress,
        action: ApparatusQueueAction::Pause,
        to: ApparatusQueueOrderState::Paused,
    },
    QueueStateTransition {
        from: ApparatusQueueOrderState::InProgress,
        action: ApparatusQueueAction::Freeze,
        to: ApparatusQueueOrderState::Frozen,
    },
    QueueStateTransition {
        from: ApparatusQueueOrderState::InProgress,
        action: ApparatusQueueAction::DetachRoll,
        // `paused` remains the compatibility queue slot state. The execution
        // records carry the canonical `roll_detached` status.
        to: ApparatusQueueOrderState::Paused,
    },
    QueueStateTransition {
        from: ApparatusQueueOrderState::Paused,
        action: ApparatusQueueAction::Resume,
        to: ApparatusQueueOrderState::InProgress,
    },
    QueueStateTransition {
        from: ApparatusQueueOrderState::InProgress,
        action: ApparatusQueueAction::Merge,
        to: ApparatusQueueOrderState::InProgress,
    },
    QueueStateTransition {
        from: ApparatusQueueOrderState::InProgress,
        action: ApparatusQueueAction::RollComplete,
        to: ApparatusQueueOrderState::InProgress,
    },
    QueueStateTransition {
        from: ApparatusQueueOrderState::InProgress,
        action: ApparatusQueueAction::Complete,
        to: ApparatusQueueOrderState::Completed,
    },
];

pub fn next_queue_state(
    current: ApparatusQueueOrderState,
    action: ApparatusQueueAction,
) -> Result<ApparatusQueueOrderState, ProductionMapError> {
    QUEUE_STATE_TRANSITIONS
        .iter()
        .find(|transition| transition.from == current && transition.action == action)
        .map(|transition| transition.to)
        .ok_or(ProductionMapError::QueueActionNotAllowed)
}
