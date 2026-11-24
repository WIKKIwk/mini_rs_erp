//! Bounded, deterministic recovery through explicitly open Wi-Fi access points.
//!
//! Adapters own association details. This policy only switches after confirmed
//! loss of real internet access, and verifies access again after association.
//! Waiting is represented by deadlines so network recovery cannot stall the
//! regular database, memory, or disk checks.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod linux;
pub mod macos;
pub mod probe;
pub(crate) mod runtime;

#[cfg(test)]
mod tests;

const MAX_EVENTS: usize = 16;
const MAX_COOLDOWNS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenNetwork {
    pub ssid: String,
    pub bssid: String,
    pub signal: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WifiError {
    #[error("unsupported")]
    Unsupported,
    #[error("unavailable")]
    Unavailable,
    #[error("permission_denied")]
    PermissionDenied,
    #[error("command_failed")]
    CommandFailed,
    #[error("invalid_output")]
    InvalidOutput,
    #[error("timed_out")]
    TimedOut,
    #[error("invalid_configuration")]
    InvalidConfiguration,
}

impl WifiError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::PermissionDenied => "permission_denied",
            Self::CommandFailed => "command_failed",
            Self::InvalidOutput => "invalid_output",
            Self::TimedOut => "timed_out",
            Self::InvalidConfiguration => "invalid_configuration",
        }
    }

    fn permanent(self) -> bool {
        matches!(
            self,
            Self::Unsupported | Self::PermissionDenied | Self::InvalidConfiguration
        )
    }
}

#[async_trait]
pub trait WifiAdapter: Send + Sync {
    /// Opaque token for restoring the connection present before recovery.
    async fn current_connection(&self) -> Result<Option<String>, WifiError>;
    /// Only access points positively identified as requiring no credentials.
    async fn scan_open(&self) -> Result<Vec<OpenNetwork>, WifiError>;
    async fn connect_open(&self, network: &OpenNetwork) -> Result<(), WifiError>;
    /// Disconnect and remove only the connection created by this adapter.
    async fn disconnect_attempt(&self) -> Result<(), WifiError>;
    async fn restore_connection(&self, token: &str) -> Result<(), WifiError>;
}

#[async_trait]
pub trait InternetProbe: Send + Sync {
    /// Association, DNS answers, and captive-portal redirects are insufficient.
    async fn has_internet(&self) -> Result<bool, WifiError>;
}

#[derive(Debug, Clone)]
pub struct WifiConfig {
    pub enabled: bool,
    pub interface: Option<String>,
    pub check_interval: Duration,
    pub failure_samples: u32,
    pub settle_delay: Duration,
    pub retry_delay: Duration,
    pub network_cooldown: Duration,
    pub round_cooldown: Duration,
    pub max_networks_per_round: usize,
}

impl Default for WifiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interface: None,
            check_interval: Duration::from_secs(15),
            failure_samples: 3,
            settle_delay: Duration::from_secs(5),
            retry_delay: Duration::from_secs(5),
            network_cooldown: Duration::from_secs(300),
            round_cooldown: Duration::from_secs(60),
            max_networks_per_round: 5,
        }
    }
}

impl WifiConfig {
    pub fn from_env() -> Result<Self, WifiError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Result<Self, WifiError> {
        let defaults = Self::default();
        let enabled = match lookup("MINI_ERP_DOCTOR_WIFI_ENABLED") {
            None => defaults.enabled,
            Some(value) => match value.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => true,
                "false" | "0" | "no" | "off" => false,
                _ => return Err(WifiError::InvalidConfiguration),
            },
        };
        let interface = lookup("MINI_ERP_DOCTOR_WIFI_INTERFACE")
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let mut seconds = |name: &str, fallback: Duration| -> Result<Duration, WifiError> {
            lookup(name)
                .map(|value| {
                    value
                        .trim()
                        .parse::<u64>()
                        .map(Duration::from_secs)
                        .map_err(|_| WifiError::InvalidConfiguration)
                })
                .unwrap_or(Ok(fallback))
        };
        let check_interval = seconds(
            "MINI_ERP_DOCTOR_WIFI_CHECK_INTERVAL_SECONDS",
            defaults.check_interval,
        )?;
        let settle_delay = seconds(
            "MINI_ERP_DOCTOR_WIFI_SETTLE_DELAY_SECONDS",
            defaults.settle_delay,
        )?;
        let retry_delay = seconds(
            "MINI_ERP_DOCTOR_WIFI_RETRY_DELAY_SECONDS",
            defaults.retry_delay,
        )?;
        let network_cooldown = seconds(
            "MINI_ERP_DOCTOR_WIFI_NETWORK_COOLDOWN_SECONDS",
            defaults.network_cooldown,
        )?;
        let round_cooldown = seconds(
            "MINI_ERP_DOCTOR_WIFI_ROUND_COOLDOWN_SECONDS",
            defaults.round_cooldown,
        )?;
        let failure_samples = lookup("MINI_ERP_DOCTOR_WIFI_FAILURE_SAMPLES")
            .map(|value| {
                value
                    .trim()
                    .parse()
                    .map_err(|_| WifiError::InvalidConfiguration)
            })
            .unwrap_or(Ok(defaults.failure_samples))?;
        let max_networks_per_round = lookup("MINI_ERP_DOCTOR_WIFI_MAX_NETWORKS_PER_ROUND")
            .map(|value| {
                value
                    .trim()
                    .parse()
                    .map_err(|_| WifiError::InvalidConfiguration)
            })
            .unwrap_or(Ok(defaults.max_networks_per_round))?;
        let config = Self {
            enabled,
            interface,
            check_interval,
            failure_samples,
            settle_delay,
            retry_delay,
            network_cooldown,
            round_cooldown,
            max_networks_per_round,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), WifiError> {
        let durations = [
            (self.check_interval, 3600),
            (self.settle_delay, 300),
            (self.retry_delay, 300),
            (self.network_cooldown, 86400),
            (self.round_cooldown, 600),
        ];
        if durations.iter().any(|(duration, max)| {
            *duration < Duration::from_secs(1) || *duration > Duration::from_secs(*max)
        }) || !(1..=60).contains(&self.failure_samples)
            || !(1..=32).contains(&self.max_networks_per_round)
            || self.interface.as_ref().is_some_and(|name| {
                name.is_empty()
                    || name.len() > 64
                    || name.starts_with('-')
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
            })
        {
            return Err(WifiError::InvalidConfiguration);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WifiRecoveryReport {
    pub enabled: bool,
    pub status: String,
    pub failures: u32,
    /// Connection attempts during the current incident, reset after recovery.
    pub attempts: u32,
    pub current_ssid: Option<String>,
    /// Static codes only; never raw command output or restoration tokens.
    pub last_error: Option<String>,
    pub checked_at_unix: i64,
    pub next_attempt_at_unix: Option<i64>,
    pub events: Vec<WifiRecoveryEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WifiRecoveryEvent {
    pub status: String,
    pub code: Option<String>,
    pub at_unix: i64,
}

#[derive(Clone, Copy)]
enum Resume {
    Cooldown(Option<WifiError>),
    Suspend(WifiError),
}

#[derive(Clone, Copy)]
enum CleanupNext {
    TryNext,
    Finish(Resume),
}

enum Phase {
    Monitoring,
    Trying,
    Settling(OpenNetwork),
    Cleaning(CleanupNext),
    Restoring(Resume),
    Cooldown,
    Suspended,
}

struct NetworkCooldown {
    until: Instant,
    /// A successful scan has stopped seeing this AP since its last failure.
    absent: bool,
}

pub struct WifiRecovery {
    config: WifiConfig,
    adapter: Arc<dyn WifiAdapter>,
    probe: Arc<dyn InternetProbe>,
    phase: Phase,
    report: WifiRecoveryReport,
    next_due: Option<Instant>,
    candidates: VecDeque<OpenNetwork>,
    cooldowns: HashMap<String, NetworkCooldown>,
    /// Kept across successful fallback connections until the baseline is restored.
    baseline: Option<Option<String>>,
    owns_connection: bool,
    round_mutated: bool,
    failed_rounds: u32,
}

include!("wifi_parts/connection_state.rs");
