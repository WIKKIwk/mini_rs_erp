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
        let value = value.trim();
        if value.eq_ignore_ascii_case("pending") {
            Some(Self::Pending)
        } else if value.eq_ignore_ascii_case("in_progress") {
            Some(Self::InProgress)
        } else if value.eq_ignore_ascii_case("paused") {
            Some(Self::Paused)
        } else if value.eq_ignore_ascii_case("frozen") {
            Some(Self::Frozen)
        } else if value.eq_ignore_ascii_case("completed") {
            Some(Self::Completed)
        } else {
            None
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
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("start") {
            Some(Self::Start)
        } else if value.eq_ignore_ascii_case("pause") {
            Some(Self::Pause)
        } else if value.eq_ignore_ascii_case("freeze") {
            Some(Self::Freeze)
        } else if value.eq_ignore_ascii_case("detach_roll") {
            Some(Self::DetachRoll)
        } else if value.eq_ignore_ascii_case("resume") {
            Some(Self::Resume)
        } else if value.eq_ignore_ascii_case("merge") {
            Some(Self::Merge)
        } else if value.eq_ignore_ascii_case("roll_complete") {
            Some(Self::RollComplete)
        } else if value.eq_ignore_ascii_case("complete") {
            Some(Self::Complete)
        } else {
            None
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Pause => "pause",
            Self::Freeze => "freeze",
            Self::DetachRoll => "detach_roll",
            Self::Resume => "resume",
            Self::Merge => "merge",
            Self::RollComplete => "roll_complete",
            Self::Complete => "complete",
        }
    }

    pub const fn creates_resumable_output(self) -> bool {
        matches!(self, Self::Pause | Self::DetachRoll)
    }

    pub const fn records_progress_output(self) -> bool {
        matches!(
            self,
            Self::Pause | Self::DetachRoll | Self::RollComplete | Self::Complete
        )
    }
}

pub fn next_queue_state(
    current: ApparatusQueueOrderState,
    action: ApparatusQueueAction,
) -> Result<ApparatusQueueOrderState, ProductionMapError> {
    match (current, action) {
        (ApparatusQueueOrderState::Pending, ApparatusQueueAction::Start) => {
            Ok(ApparatusQueueOrderState::InProgress)
        }
        (ApparatusQueueOrderState::InProgress, ApparatusQueueAction::Pause) => {
            Ok(ApparatusQueueOrderState::Paused)
        }
        (ApparatusQueueOrderState::InProgress, ApparatusQueueAction::Freeze) => {
            Ok(ApparatusQueueOrderState::Frozen)
        }
        // Queue transport retains `paused`; execution records carry the
        // canonical `roll_detached` status.
        (ApparatusQueueOrderState::InProgress, ApparatusQueueAction::DetachRoll) => {
            Ok(ApparatusQueueOrderState::Paused)
        }
        (ApparatusQueueOrderState::Paused, ApparatusQueueAction::Resume) => {
            Ok(ApparatusQueueOrderState::InProgress)
        }
        (ApparatusQueueOrderState::InProgress, ApparatusQueueAction::Merge)
        | (ApparatusQueueOrderState::InProgress, ApparatusQueueAction::RollComplete) => {
            Ok(ApparatusQueueOrderState::InProgress)
        }
        (ApparatusQueueOrderState::InProgress, ApparatusQueueAction::Complete) => {
            Ok(ApparatusQueueOrderState::Completed)
        }
        _ => Err(ProductionMapError::QueueActionNotAllowed),
    }
}
