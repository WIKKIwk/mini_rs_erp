//! Deterministic health policy inspired by Monit's repeated-check model.
//!
//! A failed sample starts an incident only after confirmation. Resource incidents
//! remain latched until repeated recovery samples cross a separate threshold.
//! Missing samples never resolve an incident. This module makes policy decisions;
//! it does not restart services, kill processes, or delete files.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::core::admin::models::{AdminServerMonitorDatabase, AdminServerMonitorRuntime};

const MAX_EVENTS: usize = 32;

pub mod wifi;

#[derive(Debug, Clone)]
pub struct DoctorPolicy {
    pub failure_samples: u32,
    pub recovery_samples: u32,
    pub memory_critical_percent: i64,
    pub memory_recovery_percent: i64,
    pub disk_critical_available_mb: i64,
    pub disk_recovery_available_mb: i64,
}

impl Default for DoctorPolicy {
    fn default() -> Self {
        Self {
            failure_samples: 3,
            recovery_samples: 2,
            memory_critical_percent: 90,
            memory_recovery_percent: 80,
            disk_critical_available_mb: 1024,
            disk_recovery_available_mb: 1536,
        }
    }
}

impl DoctorPolicy {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            failure_samples: env_value("MINI_ERP_DOCTOR_FAILURE_SAMPLES", defaults.failure_samples),
            recovery_samples: env_value(
                "MINI_ERP_DOCTOR_RECOVERY_SAMPLES",
                defaults.recovery_samples,
            ),
            memory_critical_percent: env_value(
                "MINI_ERP_DOCTOR_MEMORY_CRITICAL_PERCENT",
                defaults.memory_critical_percent,
            ),
            memory_recovery_percent: env_value(
                "MINI_ERP_DOCTOR_MEMORY_RECOVERY_PERCENT",
                defaults.memory_recovery_percent,
            ),
            disk_critical_available_mb: env_value(
                "MINI_ERP_DOCTOR_DISK_CRITICAL_AVAILABLE_MB",
                defaults.disk_critical_available_mb,
            ),
            disk_recovery_available_mb: env_value(
                "MINI_ERP_DOCTOR_DISK_RECOVERY_AVAILABLE_MB",
                defaults.disk_recovery_available_mb,
            ),
        }
        .normalized()
    }

    fn normalized(mut self) -> Self {
        self.failure_samples = self.failure_samples.clamp(1, 60);
        self.recovery_samples = self.recovery_samples.clamp(1, 60);
        self.memory_critical_percent = self.memory_critical_percent.clamp(1, 100);
        self.memory_recovery_percent = self
            .memory_recovery_percent
            .clamp(0, self.memory_critical_percent - 1);
        self.disk_critical_available_mb = self.disk_critical_available_mb.clamp(0, i64::MAX - 1);
        self.disk_recovery_available_mb = self
            .disk_recovery_available_mb
            .max(self.disk_critical_available_mb + 1);
        self
    }
}

fn env_value<T: std::str::FromStr>(name: &str, fallback: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DoctorReport {
    pub status: String,
    /// Pause new backups while a memory or disk incident is confirmed.
    pub protected: bool,
    pub checked_at_unix: i64,
    pub checks: Vec<DoctorCheck>,
    /// Recent transitions, oldest first. No raw database errors are retained.
    pub events: Vec<DoctorEvent>,
    pub wifi: wifi::WifiRecoveryReport,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DoctorCheck {
    pub id: String,
    pub status: String,
    pub message: String,
    pub value: Option<i64>,
    pub unit: String,
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    /// A confirmed incident remains active during recovery or missing samples.
    pub critical: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DoctorEvent {
    pub rule_id: String,
    pub from_status: String,
    pub to_status: String,
    pub at_unix: i64,
    pub message: String,
}

#[derive(Clone)]
pub struct SystemDoctor {
    inner: Arc<Mutex<DoctorState>>,
}

struct DoctorState {
    policy: DoctorPolicy,
    report: DoctorReport,
}

#[derive(Clone, Copy)]
enum Sample {
    Failed(i64),
    Good(i64),
    /// Between the incident and recovery thresholds.
    Neutral(i64),
    Unknown,
}

impl Default for SystemDoctor {
    fn default() -> Self {
        Self::new(DoctorPolicy::default())
    }
}

impl SystemDoctor {
    pub(crate) fn publish_wifi(&self, wifi: wifi::WifiRecoveryReport) {
        let mut state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        if state.report.wifi.status != wifi.status {
            let event = DoctorEvent {
                rule_id: "wifi".to_string(),
                from_status: state.report.wifi.status.clone(),
                to_status: wifi.status.clone(),
                at_unix: if wifi.checked_at_unix > 0 {
                    wifi.checked_at_unix
                } else {
                    time::OffsetDateTime::now_utc().unix_timestamp()
                },
                message: format!("Wi-Fi recovery status: {}", wifi.status),
            };
            tracing::info!(status = %wifi.status, "system doctor Wi-Fi recovery transition");
            state.report.events.push(event);
            if state.report.events.len() > MAX_EVENTS {
                state.report.events.remove(0);
            }
        }
        state.report.wifi = wifi;
    }

    pub fn new(policy: DoctorPolicy) -> Self {
        let checks = [("database", "reachable"), ("memory", "%"), ("disk", "MiB")]
            .into_iter()
            .map(|(id, unit)| DoctorCheck {
                id: id.to_string(),
                status: "starting".to_string(),
                message: "Waiting for the first health sample.".to_string(),
                unit: unit.to_string(),
                ..DoctorCheck::default()
            })
            .collect();
        Self {
            inner: Arc::new(Mutex::new(DoctorState {
                policy: policy.normalized(),
                report: DoctorReport {
                    status: "starting".to_string(),
                    checks,
                    ..DoctorReport::default()
                },
            })),
        }
    }

    pub fn observe(
        &self,
        database: &AdminServerMonitorDatabase,
        runtime: &AdminServerMonitorRuntime,
        now_unix: i64,
    ) -> DoctorReport {
        let mut state = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let policy = state.policy.clone();
        let database_sample = if !database.configured {
            Sample::Unknown
        } else if database.reachable {
            Sample::Good(1)
        } else {
            Sample::Failed(0)
        };
        let memory_sample =
            if runtime.memory_total_mb <= 0 || !(0..=100).contains(&runtime.memory_percent) {
                Sample::Unknown
            } else if runtime.memory_percent >= policy.memory_critical_percent {
                Sample::Failed(runtime.memory_percent)
            } else if runtime.memory_percent <= policy.memory_recovery_percent {
                Sample::Good(runtime.memory_percent)
            } else {
                Sample::Neutral(runtime.memory_percent)
            };
        let disk_sample = if runtime.disk_total_mb <= 0
            || runtime.disk_available_mb < 0
            || runtime.disk_available_mb > runtime.disk_total_mb
        {
            Sample::Unknown
        } else if runtime.disk_available_mb <= policy.disk_critical_available_mb {
            Sample::Failed(runtime.disk_available_mb)
        } else if runtime.disk_available_mb >= policy.disk_recovery_available_mb {
            Sample::Good(runtime.disk_available_mb)
        } else {
            Sample::Neutral(runtime.disk_available_mb)
        };

        for (index, sample) in [database_sample, memory_sample, disk_sample]
            .into_iter()
            .enumerate()
        {
            let check = &mut state.report.checks[index];
            let previous_status = check.status.clone();
            update_check(check, sample, &policy);
            if previous_status != check.status {
                let event = DoctorEvent {
                    rule_id: check.id.clone(),
                    from_status: previous_status,
                    to_status: check.status.clone(),
                    at_unix: now_unix,
                    message: check.message.clone(),
                };
                tracing::info!(
                    rule_id = %event.rule_id,
                    from_status = %event.from_status,
                    to_status = %event.to_status,
                    "system doctor health transition"
                );
                state.report.events.push(event);
            }
        }

        let report = &mut state.report;
        if report.events.len() > MAX_EVENTS {
            report.events.drain(..report.events.len() - MAX_EVENTS);
        }
        report.protected = report
            .checks
            .iter()
            .any(|check| check.critical && matches!(check.id.as_str(), "memory" | "disk"));
        report.status = aggregate_status(&report.checks).to_string();
        report.checked_at_unix = now_unix;
        report.clone()
    }

    pub fn snapshot(&self) -> DoctorReport {
        self.inner
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .report
            .clone()
    }

    pub fn protected(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .report
            .protected
    }
}

fn update_check(check: &mut DoctorCheck, sample: Sample, policy: &DoctorPolicy) {
    let status = match sample {
        Sample::Unknown => {
            check.value = None;
            check.consecutive_failures = 0;
            check.consecutive_successes = 0;
            "unknown"
        }
        Sample::Failed(value) => {
            check.value = Some(value);
            check.consecutive_successes = 0;
            check.consecutive_failures = check.consecutive_failures.saturating_add(1);
            if check.consecutive_failures >= policy.failure_samples {
                check.critical = true;
            }
            if check.critical {
                "critical"
            } else {
                "warning"
            }
        }
        Sample::Good(value) => {
            check.value = Some(value);
            check.consecutive_failures = 0;
            if check.critical {
                check.consecutive_successes = check.consecutive_successes.saturating_add(1);
                if check.consecutive_successes >= policy.recovery_samples {
                    check.critical = false;
                    "healthy"
                } else {
                    "recovering"
                }
            } else {
                check.consecutive_successes = 0;
                "healthy"
            }
        }
        Sample::Neutral(value) => {
            check.value = Some(value);
            check.consecutive_failures = 0;
            check.consecutive_successes = 0;
            if check.critical {
                "critical"
            } else {
                "healthy"
            }
        }
    };
    check.status = status.to_string();
    let subject = match check.id.as_str() {
        "database" => "Database connectivity",
        "memory" => "Memory pressure",
        _ => "Disk space pressure",
    };
    check.message = match status {
        "unknown" => format!("{subject} is unknown; any confirmed incident remains active."),
        "warning" => format!(
            "{subject}: confirming failure ({}/{} samples).",
            check.consecutive_failures, policy.failure_samples
        ),
        "critical" => format!("{subject}: confirmed incident; waiting for recovery."),
        "recovering" => format!(
            "{subject}: confirming recovery ({}/{} samples).",
            check.consecutive_successes, policy.recovery_samples
        ),
        _ => format!("{subject} is healthy."),
    };
    if let Some(value) = check.value {
        check
            .message
            .push_str(&format!(" Observed: {value} {}.", check.unit));
    }
}

fn aggregate_status(checks: &[DoctorCheck]) -> &'static str {
    let critical = checks.iter().filter(|check| check.critical);
    if critical.clone().any(|check| check.status != "recovering") {
        "critical"
    } else if critical.count() > 0 {
        "recovering"
    } else if checks.iter().any(|check| check.status == "warning") {
        "warning"
    } else if checks.iter().any(|check| check.status == "unknown") {
        "unknown"
    } else if checks.iter().any(|check| check.status == "starting") {
        "starting"
    } else {
        "healthy"
    }
}

#[cfg(test)]
mod tests;
