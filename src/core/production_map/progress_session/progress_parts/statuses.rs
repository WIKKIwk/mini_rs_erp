#[inline]
fn parse_ascii_variant<T: Copy>(value: &str, variants: &[(&str, T)]) -> Option<T> {
    let value = value.trim();
    variants.iter().find_map(|(name, variant)| {
        value.eq_ignore_ascii_case(name).then_some(*variant)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderRunStatus {
    Active,
    Paused,
    Frozen,
    RollDetached,
    Completed,
}

impl OrderRunStatus {
    pub fn parse(value: &str) -> Option<Self> {
        parse_ascii_variant(
            value,
            &[
                ("active", Self::Active),
                ("paused", Self::Paused),
                ("frozen", Self::Frozen),
                ("roll_detached", Self::RollDetached),
                ("completed", Self::Completed),
            ],
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Frozen => "frozen",
            Self::RollDetached => "roll_detached",
            Self::Completed => "completed",
        }
    }

    pub fn is_open(self) -> bool {
        matches!(
            self,
            Self::Active | Self::Paused | Self::Frozen | Self::RollDetached
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderProgressBatchStatus {
    Paused,
    RollDetached,
    Completed,
    Resumed,
}

impl OrderProgressBatchStatus {
    pub fn parse(value: &str) -> Option<Self> {
        parse_ascii_variant(
            value,
            &[
                ("paused", Self::Paused),
                ("roll_detached", Self::RollDetached),
                ("completed", Self::Completed),
                ("resumed", Self::Resumed),
            ],
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::RollDetached => "roll_detached",
            Self::Completed => "completed",
            Self::Resumed => "resumed",
        }
    }

    pub const fn is_resumable(self) -> bool {
        matches!(self, Self::Paused | Self::RollDetached)
    }
}

#[cfg(test)]
mod order_progress_batch_status_tests {
    use super::OrderProgressBatchStatus;

    #[test]
    fn resumable_status_classification_is_canonical() {
        assert!(OrderProgressBatchStatus::Paused.is_resumable());
        assert!(OrderProgressBatchStatus::RollDetached.is_resumable());
        assert!(!OrderProgressBatchStatus::Completed.is_resumable());
        assert!(!OrderProgressBatchStatus::Resumed.is_resumable());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderProgressBatchWipStatus {
    Waiting,
    InUse,
    Processed,
}

impl OrderProgressBatchWipStatus {
    pub fn parse(value: &str) -> Option<Self> {
        parse_ascii_variant(
            value,
            &[
                ("waiting", Self::Waiting),
                ("in_use", Self::InUse),
                ("processed", Self::Processed),
            ],
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::InUse => "in_use",
            Self::Processed => "processed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderRunInputSourceKind {
    ProgressBatch,
    OpeningWip,
}

impl OrderRunInputSourceKind {
    pub fn parse(value: &str) -> Option<Self> {
        parse_ascii_variant(
            value,
            &[
                ("progress_batch", Self::ProgressBatch),
                ("opening_wip", Self::OpeningWip),
            ],
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProgressBatch => "progress_batch",
            Self::OpeningWip => "opening_wip",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderRunInputStatus {
    InUse,
    Processed,
}

impl OrderRunInputStatus {
    pub fn parse(value: &str) -> Option<Self> {
        parse_ascii_variant(
            value,
            &[("in_use", Self::InUse), ("processed", Self::Processed)],
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::InUse => "in_use",
            Self::Processed => "processed",
        }
    }
}
