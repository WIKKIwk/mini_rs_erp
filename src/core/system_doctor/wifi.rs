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

impl WifiRecovery {
    pub fn new(
        config: WifiConfig,
        adapter: Arc<dyn WifiAdapter>,
        probe: Arc<dyn InternetProbe>,
    ) -> Self {
        let error = config.validate().err();
        let status = if !config.enabled {
            "disabled"
        } else if error.is_some() {
            "configuration_error"
        } else {
            "starting"
        };
        let report = WifiRecoveryReport {
            enabled: config.enabled,
            status: status.to_owned(),
            last_error: error.map(|error| error.code().to_owned()),
            ..WifiRecoveryReport::default()
        };
        Self {
            config,
            adapter,
            probe,
            phase: if error.is_some() {
                Phase::Suspended
            } else {
                Phase::Monitoring
            },
            report,
            next_due: None,
            candidates: VecDeque::new(),
            cooldowns: HashMap::new(),
            baseline: None,
            owns_connection: false,
            round_mutated: false,
            failed_rounds: 0,
        }
    }

    pub fn report(&self) -> WifiRecoveryReport {
        self.report.clone()
    }

    /// Advance a single deadline without sleeping. At most one new AP is tried.
    pub async fn step(&mut self, now: Instant, now_unix: i64) -> WifiRecoveryReport {
        if !self.config.enabled
            || matches!(self.phase, Phase::Suspended)
            || self.next_due.is_some_and(|deadline| now < deadline)
        {
            return self.report();
        }
        let started = Instant::now();
        self.report.checked_at_unix = now_unix;
        self.next_due = None;
        self.report.next_attempt_at_unix = None;
        match std::mem::replace(&mut self.phase, Phase::Monitoring) {
            Phase::Monitoring => match self.probe.has_internet().await {
                Ok(true) => self.online(now, now_unix),
                Ok(false) => {
                    self.report.failures = self.report.failures.saturating_add(1);
                    self.report.last_error = None;
                    if self.report.failures < self.config.failure_samples {
                        self.transition("suspected_offline", now_unix);
                        self.schedule(now, now_unix, self.config.check_interval);
                    } else {
                        self.begin_round(now, now_unix).await;
                    }
                }
                Err(error) => {
                    // An unknown sample breaks the consecutive-failure sequence.
                    self.report.failures = 0;
                    self.probe_error(error, now, now_unix, Phase::Monitoring);
                }
            },
            Phase::Cooldown => match self.probe.has_internet().await {
                Ok(true) => self.online(now, now_unix),
                Ok(false) => self.begin_round(now, now_unix).await,
                Err(error) => self.probe_error(error, now, now_unix, Phase::Cooldown),
            },
            Phase::Trying => self.try_next(now, now_unix).await,
            Phase::Settling(network) => match self.probe.has_internet().await {
                Ok(true) => self.online(now, now_unix),
                Ok(false) => {
                    self.mark_failed(&network, now);
                    self.report.last_error = Some("no_internet".to_owned());
                    self.phase = Phase::Cleaning(CleanupNext::TryNext);
                    self.cleanup(CleanupNext::TryNext, now, now_unix).await;
                }
                Err(error) => {
                    self.report.last_error = Some(error.code().to_owned());
                    let resume = if error.permanent() {
                        Resume::Suspend(error)
                    } else {
                        Resume::Cooldown(Some(error))
                    };
                    self.cleanup(CleanupNext::Finish(resume), now, now_unix)
                        .await;
                }
            },
            Phase::Cleaning(next) => self.cleanup(next, now, now_unix).await,
            Phase::Restoring(resume) => self.restore(resume, now, now_unix).await,
            Phase::Suspended => self.phase = Phase::Suspended,
        }
        // Commands and HTTPS probes can take several seconds. Start the next
        // settle/retry period after they finish, including with an injected clock.
        let elapsed = started.elapsed();
        if let Some(deadline) = self.next_due.as_mut() {
            if let Some(completed_deadline) = deadline.checked_add(elapsed) {
                *deadline = completed_deadline;
            }
            if let Some(unix) = self.report.next_attempt_at_unix.as_mut() {
                *unix = unix.saturating_add(elapsed.as_secs().min(i64::MAX as u64) as i64);
            }
        }
        self.report()
    }

    async fn begin_round(&mut self, now: Instant, now_unix: i64) {
        // A retained baseline also represents an earlier best-effort restore
        // which could not reach its AP. Keep using that original token.
        self.round_mutated = self.owns_connection || self.baseline.is_some();
        if self.baseline.is_none() {
            match self.adapter.current_connection().await {
                Ok(connection) => self.baseline = Some(connection),
                Err(error) => {
                    self.fail_round(error, now, now_unix);
                    return;
                }
            }
        }
        match self.adapter.scan_open().await {
            Ok(networks) => {
                let mut networks: Vec<_> = networks.into_iter().filter_map(valid_network).collect();
                let visible: std::collections::HashSet<_> = networks
                    .iter()
                    .map(|network| network.bssid.as_str())
                    .collect();
                self.cooldowns.retain(|bssid, cooldown| {
                    if now >= cooldown.until {
                        return false;
                    }
                    if visible.contains(bssid.as_str()) {
                        // Reappearing APs may have recovered after a restart.
                        !cooldown.absent
                    } else {
                        cooldown.absent = true;
                        true
                    }
                });
                networks.retain(|network| !self.cooldowns.contains_key(&network.bssid));
                networks.sort_by(|left, right| {
                    right
                        .signal
                        .cmp(&left.signal)
                        .then_with(|| left.bssid.cmp(&right.bssid))
                });
                let mut seen = std::collections::HashSet::new();
                self.candidates = networks
                    .into_iter()
                    .filter(|network| seen.insert(network.bssid.clone()))
                    .take(self.config.max_networks_per_round)
                    .collect();
                self.report.last_error = None;
                if self.candidates.is_empty() {
                    self.finish_round(Resume::Cooldown(None), now, now_unix);
                } else {
                    self.phase = Phase::Trying;
                    self.transition("scanning", now_unix);
                    self.schedule(now, now_unix, Duration::ZERO);
                }
            }
            Err(error) => self.fail_round(error, now, now_unix),
        }
    }

    async fn try_next(&mut self, now: Instant, now_unix: i64) {
        // A recovered wired route or an administrator action must stop switching.
        match self.probe.has_internet().await {
            Ok(true) => {
                self.online(now, now_unix);
                return;
            }
            Err(error) => {
                self.report.last_error = Some(error.code().to_owned());
                let resume = if error.permanent() {
                    Resume::Suspend(error)
                } else {
                    Resume::Cooldown(Some(error))
                };
                self.finish_round(resume, now, now_unix);
                return;
            }
            Ok(false) => {}
        }
        let Some(network) = self.candidates.pop_front() else {
            self.finish_round(Resume::Cooldown(None), now, now_unix);
            return;
        };
        self.report.attempts = self.report.attempts.saturating_add(1);
        self.report.current_ssid = Some(network.ssid.clone());
        self.owns_connection = true; // Failed commands can still partially associate.
        self.round_mutated = true;
        match self.adapter.connect_open(&network).await {
            Ok(()) => {
                self.phase = Phase::Settling(network);
                self.report.last_error = None;
                self.transition("verifying", now_unix);
                self.schedule(now, now_unix, self.config.settle_delay);
            }
            Err(error) => {
                self.mark_failed(&network, now);
                self.report.last_error = Some(error.code().to_owned());
                let next = if error.permanent() {
                    CleanupNext::Finish(Resume::Suspend(error))
                } else {
                    CleanupNext::TryNext
                };
                self.cleanup(next, now, now_unix).await;
            }
        }
    }

    async fn cleanup(&mut self, next: CleanupNext, now: Instant, now_unix: i64) {
        match self.adapter.disconnect_attempt().await {
            Ok(()) => {
                self.owns_connection = false;
                self.report.current_ssid = None;
                match next {
                    CleanupNext::TryNext if !self.candidates.is_empty() => {
                        self.phase = Phase::Trying;
                        self.transition("waiting", now_unix);
                        self.schedule(now, now_unix, self.config.retry_delay);
                    }
                    CleanupNext::TryNext => {
                        self.finish_round(Resume::Cooldown(None), now, now_unix);
                        // Wait before restoring the original connection too.
                        if matches!(self.phase, Phase::Restoring(_)) {
                            self.schedule(now, now_unix, self.config.retry_delay);
                        }
                    }
                    CleanupNext::Finish(resume) => self.finish_round(resume, now, now_unix),
                }
            }
            Err(error) => {
                self.report.last_error = Some(error.code().to_owned());
                if error.permanent() {
                    self.suspend(error, now_unix);
                } else {
                    self.phase = Phase::Cleaning(next);
                    self.transition("cleanup_failed", now_unix);
                    self.schedule(now, now_unix, self.config.retry_delay);
                }
            }
        }
    }

    fn finish_round(&mut self, resume: Resume, now: Instant, now_unix: i64) {
        self.candidates.clear();
        if self.owns_connection {
            self.phase = Phase::Cleaning(CleanupNext::Finish(resume));
            self.transition("restoring", now_unix);
            self.schedule(now, now_unix, Duration::ZERO);
        } else if self.round_mutated && matches!(self.baseline, Some(Some(_))) {
            self.phase = Phase::Restoring(resume);
            self.transition("restoring", now_unix);
            self.schedule(now, now_unix, Duration::ZERO);
        } else {
            self.complete_round(resume, now, now_unix);
        }
    }

    async fn restore(&mut self, resume: Resume, now: Instant, now_unix: i64) {
        let Some(Some(token)) = self.baseline.as_ref() else {
            self.complete_round(resume, now, now_unix);
            return;
        };
        match self.adapter.restore_connection(token).await {
            Ok(()) => self.complete_round(resume, now, now_unix),
            Err(error) => {
                self.report.last_error = Some(error.code().to_owned());
                if error.permanent() {
                    self.suspend(error, now_unix);
                } else if let Resume::Suspend(cause) = resume {
                    self.transition("restore_failed", now_unix);
                    self.suspend(cause, now_unix);
                } else {
                    // The original AP may itself be offline or out of range.
                    // Cleanup has already completed, so allow a later bounded
                    // scan to discover other APs; retain the original token.
                    self.enter_cooldown("restore_failed", now, now_unix);
                }
            }
        }
    }

    fn complete_round(&mut self, resume: Resume, now: Instant, now_unix: i64) {
        self.baseline = None;
        self.owns_connection = false;
        self.round_mutated = false;
        self.report.current_ssid = None;
        match resume {
            Resume::Suspend(error) => self.suspend(error, now_unix),
            Resume::Cooldown(error) => {
                if let Some(error) = error {
                    self.report.last_error = Some(error.code().to_owned());
                }
                self.enter_cooldown("cooldown", now, now_unix);
            }
        }
    }

    fn fail_round(&mut self, error: WifiError, now: Instant, now_unix: i64) {
        self.report.last_error = Some(error.code().to_owned());
        let resume = if error.permanent() {
            Resume::Suspend(error)
        } else {
            Resume::Cooldown(Some(error))
        };
        self.finish_round(resume, now, now_unix);
    }

    fn probe_error(&mut self, error: WifiError, now: Instant, now_unix: i64, retry: Phase) {
        self.report.last_error = Some(error.code().to_owned());
        if error.permanent() {
            self.suspend(error, now_unix);
        } else {
            self.phase = retry;
            self.transition("unknown", now_unix);
            self.schedule(now, now_unix, self.config.check_interval);
        }
    }

    fn online(&mut self, now: Instant, now_unix: i64) {
        self.phase = Phase::Monitoring;
        self.report.failures = 0;
        self.report.attempts = 0;
        self.report.last_error = None;
        self.failed_rounds = 0;
        self.candidates.clear();
        self.cooldowns.retain(|_, cooldown| now < cooldown.until);
        if !self.owns_connection {
            self.baseline = None;
        }
        self.transition("online", now_unix);
        self.schedule(now, now_unix, self.config.check_interval);
    }

    fn suspend(&mut self, error: WifiError, now_unix: i64) {
        self.phase = Phase::Suspended;
        self.next_due = None;
        self.report.next_attempt_at_unix = None;
        self.report.last_error = Some(error.code().to_owned());
        self.transition("suspended", now_unix);
    }

    fn enter_cooldown(&mut self, status: &str, now: Instant, now_unix: i64) {
        self.phase = Phase::Cooldown;
        self.failed_rounds = self.failed_rounds.saturating_add(1);
        let multiplier = 1_u32 << self.failed_rounds.saturating_sub(1).min(10);
        let delay = self
            .config
            .round_cooldown
            .saturating_mul(multiplier)
            .min(Duration::from_secs(600));
        self.transition(status, now_unix);
        self.schedule(now, now_unix, delay);
    }

    fn schedule(&mut self, now: Instant, now_unix: i64, delay: Duration) {
        self.next_due = now.checked_add(delay);
        self.report.next_attempt_at_unix =
            Some(now_unix.saturating_add(delay.as_secs().min(i64::MAX as u64) as i64));
    }

    fn transition(&mut self, status: &str, now_unix: i64) {
        if self.report.status == status {
            return;
        }
        self.report.status = status.to_owned();
        if self.report.events.len() >= MAX_EVENTS {
            self.report.events.remove(0);
        }
        self.report.events.push(WifiRecoveryEvent {
            status: status.to_owned(),
            code: self.report.last_error.clone(),
            at_unix: now_unix,
        });
    }

    fn mark_failed(&mut self, network: &OpenNetwork, now: Instant) {
        self.cooldowns.retain(|_, cooldown| now < cooldown.until);
        if self.cooldowns.len() >= MAX_COOLDOWNS {
            if let Some(oldest) = self
                .cooldowns
                .iter()
                .min_by_key(|(_, cooldown)| cooldown.until)
                .map(|(bssid, _)| bssid.clone())
            {
                self.cooldowns.remove(&oldest);
            }
        }
        if let Some(until) = now.checked_add(self.config.network_cooldown) {
            self.cooldowns.insert(
                network.bssid.clone(),
                NetworkCooldown {
                    until,
                    absent: false,
                },
            );
        }
    }
}

fn valid_network(mut network: OpenNetwork) -> Option<OpenNetwork> {
    if network.ssid.trim().is_empty()
        || network.ssid.len() > 32
        || network.ssid.chars().any(char::is_control)
    {
        return None;
    }
    let bytes = network.bssid.as_bytes();
    if bytes.len() != 17
        || !bytes.iter().enumerate().all(|(index, byte)| {
            if index % 3 == 2 {
                *byte == b':'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
        || network.bssid == "00:00:00:00:00:00"
        || network.bssid.eq_ignore_ascii_case("ff:ff:ff:ff:ff:ff")
    {
        return None;
    }
    network.bssid.make_ascii_lowercase();
    Some(network)
}
