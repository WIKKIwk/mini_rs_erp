use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::core::apparatus_standard::ApparatusId;

use super::types::QueueActionActor;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusWorkingWindow {
    /// ISO weekday: Monday = 1, Sunday = 7.
    pub weekday: u8,
    /// Minutes after midnight, inclusive.
    pub start_minute: u16,
    /// Minutes after midnight, exclusive.
    pub end_minute: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusCapacityProfile {
    /// Canonical catalog identity. Persisted as `canonical_apparatus_id` in
    /// PostgreSQL; the legacy `apparatus_id` column is only synchronized for
    /// compatibility.
    pub apparatus_id: ApparatusId,
    /// Historical/display snapshot only. Never use this for identity.
    pub apparatus: String,
    #[serde(default = "default_capacity_slots")]
    pub capacity_slots: u16,
    #[serde(default)]
    pub setup_minutes: u32,
    #[serde(default)]
    pub cleanup_minutes: u32,
    #[serde(default = "default_efficiency_percent")]
    pub efficiency_percent: u16,
    #[serde(default = "default_finite_capacity")]
    pub finite_capacity: bool,
    #[serde(default)]
    pub working_windows: Vec<ApparatusWorkingWindow>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub capability_levels: BTreeMap<String, u16>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub updated_at_unix: i64,
}

impl ApparatusCapacityProfile {
    pub fn capability_level(&self, code: &str) -> u16 {
        let normalized = normalize_code(code);
        self.capability_levels
            .iter()
            .find(|(key, _)| normalize_code(key) == normalized)
            .map(|(_, level)| *level)
            .or_else(|| {
                self.capabilities
                    .iter()
                    .any(|item| normalize_code(item) == normalized)
                    .then_some(1)
            })
            .unwrap_or_default()
    }

    pub fn supports(&self, requirements: &[ApparatusCapabilityRequirement]) -> bool {
        requirements
            .iter()
            .all(|requirement| self.capability_level(&requirement.code) >= requirement.min_level)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusDowntime {
    pub id: String,
    /// Canonical catalog identity. The `apparatus` field is a display
    /// snapshot only.
    pub apparatus_id: ApparatusId,
    pub apparatus: String,
    pub starts_at_unix: i64,
    pub ends_at_unix: i64,
    pub reason: String,
    #[serde(default = "default_active")]
    pub active: bool,
    pub actor: QueueActionActor,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApparatusScheduleStatus {
    Planned,
    Active,
    Paused,
    Completed,
    Cancelled,
}

impl ApparatusScheduleStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "planned" => Some(Self::Planned),
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn reserves_capacity(self) -> bool {
        matches!(self, Self::Planned | Self::Active)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusCapabilityRequirement {
    pub code: String,
    #[serde(default = "default_capability_level")]
    pub min_level: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusScheduleReservation {
    pub reservation_id: String,
    pub idempotency_key: String,
    pub order_id: String,
    /// Canonical catalog identity. The `apparatus` field is a display
    /// snapshot only.
    pub apparatus_id: ApparatusId,
    pub apparatus: String,
    pub starts_at_unix: i64,
    pub ends_at_unix: i64,
    pub requested_duration_minutes: u32,
    pub reserved_duration_minutes: u32,
    pub status: ApparatusScheduleStatus,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub capability_requirements: Vec<ApparatusCapabilityRequirement>,
    #[serde(default)]
    pub actor: QueueActionActor,
    pub created_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusScheduleCandidate {
    pub apparatus_id: ApparatusId,
    pub apparatus: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusScheduleRequest {
    pub order_id: String,
    pub apparatus_id: String,
    pub apparatus: String,
    pub earliest_start_unix: i64,
    pub latest_end_unix: Option<i64>,
    pub duration_minutes: u32,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub idempotency_key: String,
    #[serde(default)]
    pub capability_requirements: Vec<ApparatusCapabilityRequirement>,
    #[serde(default)]
    pub candidate_apparatuses: Vec<ApparatusScheduleCandidate>,
    #[serde(default)]
    pub actor: QueueActionActor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusScheduleConflict {
    pub reservation_id: String,
    pub order_id: String,
    pub starts_at_unix: i64,
    pub ends_at_unix: i64,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusScheduleResult {
    pub reservation: ApparatusScheduleReservation,
    #[serde(default)]
    pub conflicts: Vec<ApparatusScheduleConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusCapacitySnapshot {
    pub profiles: Vec<ApparatusCapacityProfile>,
    pub downtimes: Vec<ApparatusDowntime>,
    pub reservations: Vec<ApparatusScheduleReservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusScheduleCancelRequest {
    pub reservation_id: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub actor: QueueActionActor,
}

fn default_active() -> bool {
    true
}

fn default_capacity_slots() -> u16 {
    1
}

fn default_efficiency_percent() -> u16 {
    100
}

fn default_finite_capacity() -> bool {
    true
}

fn default_capability_level() -> u16 {
    1
}

fn normalize_code(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}
