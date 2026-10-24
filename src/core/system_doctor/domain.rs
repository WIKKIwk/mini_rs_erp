//! Public-domain health checks and bounded Cloudflare Tunnel recovery.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::IpAddr,
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use tokio::{process::Command, time::timeout};

const MAX_EVENTS: usize = 16;

#[derive(Debug, Clone)]
pub struct DomainConfig {
    pub url: Url,
    pub local_url: Url,
    pub state_dir: Option<PathBuf>,
    pub tunnel_name: String,
    pub check_interval: Duration,
    pub failure_samples: u32,
    pub recovery_samples: u32,
    pub restart_cooldown: Duration,
}

impl DomainConfig {
    pub fn from_env() -> Result<Option<Self>, DomainError> {
        let raw_url = std::env::var("MINI_ERP_DOCTOR_DOMAIN_URL")
            .ok()
            .or_else(|| std::env::var("PUBLIC_HOSTNAME").ok());
        let Some(raw_url) = raw_url.filter(|value| !value.trim().is_empty()) else {
            return Ok(None);
        };
        let url = parse_domain_url(&raw_url)?;
        let local_url = match std::env::var("MINI_ERP_DOCTOR_DOMAIN_LOCAL_URL") {
            Ok(value) => parse_local_url(&value)?,
            Err(_) => {
                Url::parse("http://127.0.0.1:18081").map_err(|_| DomainError::Configuration)?
            }
        };
        let state_dir = std::env::var_os("MINI_ERP_DOCTOR_DOMAIN_STATE_DIR").map(PathBuf::from);
        let tunnel_name = std::env::var("MINI_ERP_DOCTOR_DOMAIN_TUNNEL_NAME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| default_tunnel_name(url.host_str().unwrap_or_default()));
        if tunnel_name.len() > 128
            || !tunnel_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        {
            return Err(DomainError::Configuration);
        }
        let config = Self {
            url,
            local_url,
            state_dir,
            tunnel_name,
            check_interval: env_seconds(
                "MINI_ERP_DOCTOR_DOMAIN_CHECK_INTERVAL_SECONDS",
                30,
                1,
                3600,
            )?,
            failure_samples: env_number("MINI_ERP_DOCTOR_DOMAIN_FAILURE_SAMPLES", 3, 1, 60)?,
            recovery_samples: env_number("MINI_ERP_DOCTOR_DOMAIN_RECOVERY_SAMPLES", 2, 1, 60)?,
            restart_cooldown: env_seconds(
                "MINI_ERP_DOCTOR_DOMAIN_RESTART_COOLDOWN_SECONDS",
                60,
                10,
                3600,
            )?,
        };
        Ok(Some(config))
    }
}

fn env_number(name: &str, default: u32, minimum: u32, maximum: u32) -> Result<u32, DomainError> {
    let value = match std::env::var(name) {
        Ok(value) => value
            .trim()
            .parse::<u32>()
            .map_err(|_| DomainError::Configuration)?,
        Err(_) => default,
    };
    if !(minimum..=maximum).contains(&value) {
        return Err(DomainError::Configuration);
    }
    Ok(value)
}

fn env_seconds(
    name: &str,
    default: u64,
    minimum: u64,
    maximum: u64,
) -> Result<Duration, DomainError> {
    let value = match std::env::var(name) {
        Ok(value) => value
            .trim()
            .parse::<u64>()
            .map_err(|_| DomainError::Configuration)?,
        Err(_) => default,
    };
    if !(minimum..=maximum).contains(&value) {
        return Err(DomainError::Configuration);
    }
    Ok(Duration::from_secs(value))
}

fn parse_domain_url(raw: &str) -> Result<Url, DomainError> {
    let raw = raw.trim();
    let absolute = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("https://{raw}")
    };
    let url = Url::parse(&absolute).map_err(|_| DomainError::Configuration)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(DomainError::Configuration);
    }
    Ok(url)
}

fn parse_local_url(raw: &str) -> Result<Url, DomainError> {
    let url = Url::parse(raw.trim()).map_err(|_| DomainError::Configuration)?;
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if !matches!(url.scheme(), "http" | "https")
        || !loopback
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(DomainError::Configuration);
    }
    Ok(url)
}

fn default_tunnel_name(host: &str) -> String {
    let slug = host
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("mini-rs-erp-{}", slug.trim_matches('-'))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("configuration_error")]
    Configuration,
    #[error("probe_failed")]
    ProbeFailed,
    #[error("domain_state_unavailable")]
    StateUnavailable,
    #[error("process_identity_mismatch")]
    ProcessIdentityMismatch,
    #[error("process_control_failed")]
    ProcessControlFailed,
    #[error("tunnel_launch_failed")]
    LaunchFailed,
    #[error("unsupported")]
    Unsupported,
}

impl DomainError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Configuration => "configuration_error",
            Self::ProbeFailed => "probe_failed",
            Self::StateUnavailable => "domain_state_unavailable",
            Self::ProcessIdentityMismatch => "process_identity_mismatch",
            Self::ProcessControlFailed => "process_control_failed",
            Self::LaunchFailed => "tunnel_launch_failed",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DomainRecoveryReport {
    pub enabled: bool,
    pub host: Option<String>,
    pub status: String,
    pub failures: u32,
    pub successes: u32,
    pub critical: bool,
    pub recovery_attempts: u32,
    pub last_error: Option<String>,
    pub checked_at_unix: i64,
    pub next_attempt_at_unix: Option<i64>,
    pub events: Vec<DomainRecoveryEvent>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DomainRecoveryEvent {
    pub status: String,
    pub code: Option<String>,
    pub at_unix: i64,
}

#[async_trait]
pub trait DomainAdapter: Send + Sync {
    async fn domain_is_healthy(&self) -> Result<bool, DomainError>;
    async fn local_service_is_healthy(&self) -> Result<bool, DomainError>;
    async fn reconnect_tunnel(&self) -> Result<(), DomainError>;
}

pub struct CloudflareDomainAdapter {
    config: DomainConfig,
    domain_client: Client,
    local_client: Client,
}

impl CloudflareDomainAdapter {
    pub fn new(config: DomainConfig) -> Result<Self, DomainError> {
        let build = |https_only| {
            Client::builder()
                .https_only(https_only)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(5))
                .build()
                .map_err(|_| DomainError::Configuration)
        };
        Ok(Self {
            config,
            domain_client: build(true)?,
            local_client: build(false)?,
        })
    }

    async fn probe(client: &Client, base: &Url) -> Result<bool, DomainError> {
        let mut endpoint = base.clone();
        endpoint.set_path("/healthz");
        endpoint.set_query(None);
        endpoint.set_fragment(None);
        let response = client
            .get(endpoint)
            .send()
            .await
            .map_err(|_| DomainError::ProbeFailed)?;
        Ok(response.status() == StatusCode::OK)
    }

    #[cfg(unix)]
    async fn restart_tunnel(&self) -> Result<(), DomainError> {
        let state_dir = self
            .config
            .state_dir
            .as_ref()
            .filter(|path| path.is_absolute())
            .ok_or(DomainError::StateUnavailable)?;
        let dir_metadata =
            fs::symlink_metadata(state_dir).map_err(|_| DomainError::StateUnavailable)?;
        if !dir_metadata.is_dir() || dir_metadata.file_type().is_symlink() {
            return Err(DomainError::StateUnavailable);
        }
        let config_path = state_dir.join("cloudflared.yml");
        let config_metadata =
            fs::symlink_metadata(&config_path).map_err(|_| DomainError::StateUnavailable)?;
        if !config_metadata.is_file() || config_metadata.file_type().is_symlink() {
            return Err(DomainError::StateUnavailable);
        }
        let pid_path = state_dir.join("cloudflared.pid");
        if let Ok(metadata) = fs::symlink_metadata(&pid_path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > 32 {
                return Err(DomainError::StateUnavailable);
            }
            let raw_pid =
                fs::read_to_string(&pid_path).map_err(|_| DomainError::StateUnavailable)?;
            let pid = raw_pid
                .trim()
                .parse::<u32>()
                .map_err(|_| DomainError::StateUnavailable)?;
            if pid <= 1 {
                return Err(DomainError::StateUnavailable);
            }
            if process_matches_cloudflared(pid, &config_path, &self.config.tunnel_name).await? {
                stop_cloudflared(pid, &config_path, &self.config.tunnel_name).await?;
            }
            fs::remove_file(&pid_path).map_err(|_| DomainError::StateUnavailable)?;
        }
        let log_path = state_dir.join("cloudflared.log");
        if fs::symlink_metadata(&log_path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink() || !metadata.is_file())
        {
            return Err(DomainError::StateUnavailable);
        }
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|_| DomainError::StateUnavailable)?;
        let stderr = log.try_clone().map_err(|_| DomainError::StateUnavailable)?;
        let mut command = Command::new("cloudflared");
        command
            .arg("tunnel")
            .arg("--config")
            .arg(&config_path)
            .arg("run")
            .arg(&self.config.tunnel_name)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr))
            .kill_on_drop(false);
        let mut child = command.spawn().map_err(|_| DomainError::LaunchFailed)?;
        let pid = child.id().ok_or(DomainError::LaunchFailed)?;
        let temporary_pid = state_dir.join(format!(".cloudflared.doctor.{pid}.pid"));
        let mut pid_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_pid)
            .map_err(|_| DomainError::StateUnavailable)?;
        writeln!(pid_file, "{pid}").map_err(|_| DomainError::StateUnavailable)?;
        pid_file
            .sync_all()
            .map_err(|_| DomainError::StateUnavailable)?;
        drop(pid_file);
        fs::rename(&temporary_pid, &pid_path).map_err(|_| DomainError::StateUnavailable)?;
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
        Ok(())
    }

    #[cfg(not(unix))]
    async fn restart_tunnel(&self) -> Result<(), DomainError> {
        Err(DomainError::Unsupported)
    }
}

#[async_trait]
impl DomainAdapter for CloudflareDomainAdapter {
    async fn domain_is_healthy(&self) -> Result<bool, DomainError> {
        Self::probe(&self.domain_client, &self.config.url).await
    }

    async fn local_service_is_healthy(&self) -> Result<bool, DomainError> {
        Self::probe(&self.local_client, &self.config.local_url).await
    }

    async fn reconnect_tunnel(&self) -> Result<(), DomainError> {
        self.restart_tunnel().await
    }
}

#[cfg(unix)]
async fn process_matches_cloudflared(
    pid: u32,
    config: &std::path::Path,
    tunnel_name: &str,
) -> Result<bool, DomainError> {
    let output = timeout(
        Duration::from_secs(3),
        Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| DomainError::ProcessControlFailed)?
    .map_err(|_| DomainError::ProcessControlFailed)?;
    if !output.status.success() {
        return Ok(false);
    }
    let command = String::from_utf8_lossy(&output.stdout);
    let config = config.to_string_lossy();
    let matches = command.contains("cloudflared")
        && command.contains("tunnel")
        && command.contains(config.as_ref())
        && command.contains("run")
        && command.contains(tunnel_name);
    if !matches {
        return Err(DomainError::ProcessIdentityMismatch);
    }
    Ok(true)
}

#[cfg(unix)]
async fn stop_cloudflared(
    pid: u32,
    config: &std::path::Path,
    tunnel_name: &str,
) -> Result<(), DomainError> {
    send_signal("-TERM", pid).await?;
    for _ in 0..20 {
        if !process_matches_cloudflared(pid, config, tunnel_name).await? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if process_matches_cloudflared(pid, config, tunnel_name).await? {
        send_signal("-KILL", pid).await?;
    }
    for _ in 0..8 {
        if !process_matches_cloudflared(pid, config, tunnel_name).await? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(DomainError::ProcessControlFailed)
}

#[cfg(unix)]
async fn send_signal(signal: &str, pid: u32) -> Result<(), DomainError> {
    let output = timeout(
        Duration::from_secs(3),
        Command::new("/bin/kill")
            .arg(signal)
            .arg(pid.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| DomainError::ProcessControlFailed)?
    .map_err(|_| DomainError::ProcessControlFailed)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(DomainError::ProcessControlFailed)
    }
}

pub struct DomainRecovery {
    config: DomainConfig,
    adapter: std::sync::Arc<dyn DomainAdapter>,
    report: DomainRecoveryReport,
    critical: bool,
    next_check: Option<Instant>,
    next_restart: Option<Instant>,
}

impl DomainRecovery {
    pub fn new(config: DomainConfig, adapter: std::sync::Arc<dyn DomainAdapter>) -> Self {
        let host = config.url.host_str().map(str::to_owned);
        Self {
            report: DomainRecoveryReport {
                enabled: true,
                host,
                status: "starting".to_owned(),
                ..DomainRecoveryReport::default()
            },
            config,
            adapter,
            critical: false,
            next_check: None,
            next_restart: None,
        }
    }

    pub fn report(&self) -> DomainRecoveryReport {
        self.report.clone()
    }

    pub async fn step(&mut self, now: Instant, now_unix: i64) -> DomainRecoveryReport {
        if self.next_check.is_some_and(|deadline| now < deadline) {
            return self.report();
        }
        self.next_check = Some(now + self.config.check_interval);
        self.report.checked_at_unix = now_unix;
        match self.adapter.domain_is_healthy().await {
            Ok(true) => self.observe_success(now_unix),
            Ok(false) => self.observe_failure(None, now, now_unix).await,
            Err(error) => self.observe_failure(Some(error), now, now_unix).await,
        }
        self.report()
    }

    fn observe_success(&mut self, now_unix: i64) {
        self.report.failures = 0;
        self.report.last_error = None;
        if self.critical {
            self.report.successes = self.report.successes.saturating_add(1);
            if self.report.successes >= self.config.recovery_samples {
                self.critical = false;
                self.report.status = "healthy".to_owned();
                self.report.successes = 0;
                self.report.next_attempt_at_unix = None;
            } else {
                self.report.status = "recovering".to_owned();
            }
        } else {
            self.report.successes = 0;
            self.report.status = "healthy".to_owned();
        }
        self.report.critical = self.critical;
        self.transition(now_unix);
    }

    async fn observe_failure(&mut self, error: Option<DomainError>, now: Instant, now_unix: i64) {
        self.report.successes = 0;
        self.report.failures = self.report.failures.saturating_add(1);
        self.report.last_error = Some(
            error
                .map(DomainError::code)
                .unwrap_or("endpoint_unhealthy")
                .to_owned(),
        );
        if self.report.failures >= self.config.failure_samples {
            self.critical = true;
            self.report.status = "critical".to_owned();
            self.maybe_reconnect(now, now_unix).await;
        } else {
            self.report.status = "warning".to_owned();
        }
        self.report.critical = self.critical;
        self.transition(now_unix);
    }

    async fn maybe_reconnect(&mut self, now: Instant, now_unix: i64) {
        if self.next_restart.is_some_and(|deadline| now < deadline) {
            return;
        }
        self.next_restart = Some(now + self.config.restart_cooldown);
        self.report.next_attempt_at_unix = Some(
            now_unix
                .saturating_add(self.config.restart_cooldown.as_secs().min(i64::MAX as u64) as i64),
        );
        match self.adapter.local_service_is_healthy().await {
            Ok(true) => {
                self.report.recovery_attempts = self.report.recovery_attempts.saturating_add(1);
                match self.adapter.reconnect_tunnel().await {
                    Ok(()) => {
                        self.report.status = "reconnecting".to_owned();
                        self.report.last_error = None;
                    }
                    Err(error) => self.report.last_error = Some(error.code().to_owned()),
                }
            }
            Ok(false) => self.report.last_error = Some("local_service_unavailable".to_owned()),
            Err(error) => self.report.last_error = Some(error.code().to_owned()),
        }
        self.transition(now_unix);
    }

    fn transition(&mut self, now_unix: i64) {
        let status = self.report.status.as_str();
        if self
            .report
            .events
            .last()
            .is_some_and(|event| event.status == status && event.code == self.report.last_error)
        {
            return;
        }
        if self.report.events.len() == MAX_EVENTS {
            self.report.events.remove(0);
        }
        self.report.events.push(DomainRecoveryEvent {
            status: self.report.status.clone(),
            code: self.report.last_error.clone(),
            at_unix: now_unix,
        });
    }
}

#[cfg(test)]
mod tests;

pub(crate) mod runtime;
