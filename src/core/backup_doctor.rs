use std::fs;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use serde_json;
use thiserror::Error;
use time::{OffsetDateTime, UtcOffset};
use tokio::process::Command;
use tokio::time::timeout;

use crate::core::admin::models::{
    AdminServerMonitorBackupSnapshot, AdminServerMonitorBackups,
};

const MANIFEST_NAME: &str = "backup-doctor.json";
const DEFAULT_HEALTH_MAX_AGE_HOURS: i64 = 30;
const DEFAULT_MAX_RUNTIME_MINUTES: u64 = 120;
const DEFAULT_MIN_AVAILABLE_MB: u64 = 1024;
const DEFAULT_AUTO_RETRY_MINUTES: i64 = 60;

#[derive(Debug, Clone)]
pub struct BackupDoctorConfig {
    pub backup_root: PathBuf,
    pub script_path: PathBuf,
    pub database_url: Option<String>,
    pub admin_database_url: Option<String>,
    pub auto_enabled: bool,
    pub schedule_hour: u8,
    pub schedule_minute: u8,
    pub utc_offset_minutes: i32,
    pub health_max_age_hours: i64,
    pub max_runtime: StdDuration,
    pub min_available_mb: u64,
    pub retention_enabled: bool,
}

impl BackupDoctorConfig {
    fn from_env() -> Self {
        let backup_root = backup_directory();
        let script_path = backup_script_path();
        let (schedule_hour, schedule_minute) = std::env::var("MINI_ERP_BACKUP_TIME")
            .ok()
            .and_then(|value| parse_clock(&value))
            .unwrap_or((2, 0));
        Self {
            backup_root,
            script_path,
            database_url: non_empty_env("MINI_ERP_DATABASE_URL"),
            admin_database_url: non_empty_env("MINI_ERP_ADMIN_DATABASE_URL"),
            auto_enabled: bool_env("MINI_ERP_AUTO_BACKUP_ENABLED", true),
            schedule_hour,
            schedule_minute,
            utc_offset_minutes: int_env("MINI_ERP_BACKUP_UTC_OFFSET_MINUTES", 300)
                .clamp(-23 * 60 - 59, 23 * 60 + 59) as i32,
            health_max_age_hours: int_env(
                "MINI_ERP_BACKUP_HEALTH_MAX_AGE_HOURS",
                DEFAULT_HEALTH_MAX_AGE_HOURS,
            )
            .max(1),
            max_runtime: StdDuration::from_secs(
                uint_env(
                    "MINI_ERP_BACKUP_MAX_RUNTIME_MINUTES",
                    DEFAULT_MAX_RUNTIME_MINUTES,
                )
                .max(1)
                    * 60,
            ),
            min_available_mb: uint_env(
                "MINI_ERP_BACKUP_MIN_AVAILABLE_MB",
                DEFAULT_MIN_AVAILABLE_MB,
            ),
            retention_enabled: bool_env("MINI_ERP_BACKUP_RETENTION_ENABLED", true),
        }
    }
}

#[derive(Clone)]
pub struct BackupDoctor {
    inner: Arc<BackupDoctorInner>,
}

struct BackupDoctorInner {
    config: BackupDoctorConfig,
    active_job: Mutex<Option<AdminServerMonitorBackupSnapshot>>,
    scheduler_started: AtomicBool,
}

#[derive(Debug, Error)]
pub enum BackupDoctorError {
    #[error("backup service is not configured")]
    NotConfigured,
    #[error("backup engine is unavailable")]
    EngineUnavailable,
    #[error("another backup is already running")]
    AlreadyRunning,
    #[error("backup snapshot not found")]
    NotFound,
    #[error("backup snapshot is not ready")]
    NotReady,
    #[error("backup storage failed")]
    Storage,
    #[error("backup runtime is unavailable")]
    RuntimeUnavailable,
}

#[derive(Debug, Clone)]
pub struct BackupArtifact {
    pub path: PathBuf,
    pub filename: String,
    pub size_bytes: u64,
}

impl BackupDoctor {
    pub fn from_env() -> Self {
        Self::new(BackupDoctorConfig::from_env())
    }

    pub fn new(config: BackupDoctorConfig) -> Self {
        let doctor = Self {
            inner: Arc::new(BackupDoctorInner {
                config,
                active_job: Mutex::new(None),
                scheduler_started: AtomicBool::new(false),
            }),
        };
        doctor.reconcile_interrupted_jobs();
        doctor
    }

    #[cfg(test)]
    pub fn for_test(
        backup_root: impl Into<PathBuf>,
        script_path: impl Into<PathBuf>,
        database_url: impl Into<String>,
    ) -> Self {
        Self::new(BackupDoctorConfig {
            backup_root: backup_root.into(),
            script_path: script_path.into(),
            database_url: Some(database_url.into()),
            admin_database_url: None,
            auto_enabled: false,
            schedule_hour: 2,
            schedule_minute: 0,
            utc_offset_minutes: 300,
            health_max_age_hours: DEFAULT_HEALTH_MAX_AGE_HOURS,
            max_runtime: StdDuration::from_secs(30),
            min_available_mb: 0,
            retention_enabled: false,
        })
    }

    pub fn start_scheduler(&self) {
        if cfg!(test)
            || !self.inner.config.auto_enabled
            || self.inner.config.database_url.is_none()
            || self
                .inner
                .scheduler_started
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            self.inner.scheduler_started.store(false, Ordering::Release);
            tracing::warn!("backup doctor scheduler could not find a Tokio runtime");
            return;
        };
        let doctor = self.clone();
        handle.spawn(async move {
            loop {
                doctor.maybe_start_scheduled_backup();
                tokio::time::sleep(StdDuration::from_secs(60)).await;
            }
        });
    }

    pub fn start_manual_backup(
        &self,
        requested_by: impl Into<String>,
    ) -> Result<AdminServerMonitorBackupSnapshot, BackupDoctorError> {
        self.start_backup("manual", requested_by.into())
    }

    pub fn report(&self, now: OffsetDateTime) -> AdminServerMonitorBackups {
        let active_job = self
            .inner
            .active_job
            .lock()
            .ok()
            .and_then(|active| active.clone());
        scan_backup_root(
            &self.inner.config.backup_root,
            now,
            self.inner.config.health_max_age_hours,
            active_job,
        )
        .report
    }

    pub fn artifact(&self, id: &str) -> Result<BackupArtifact, BackupDoctorError> {
        let id = id.trim();
        if id.is_empty() {
            return Err(BackupDoctorError::NotFound);
        }
        let scan = scan_backup_root(
            &self.inner.config.backup_root,
            OffsetDateTime::now_utc(),
            self.inner.config.health_max_age_hours,
            None,
        );
        if let Some(artifact) = scan.artifacts.get(id) {
            return Ok(artifact.clone());
        }
        if scan.report.snapshots.iter().any(|snapshot| snapshot.id == id) {
            return Err(BackupDoctorError::NotReady);
        }
        Err(BackupDoctorError::NotFound)
    }

    fn start_backup(
        &self,
        source: &str,
        requested_by: String,
    ) -> Result<AdminServerMonitorBackupSnapshot, BackupDoctorError> {
        let database_url = self
            .inner
            .config
            .database_url
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .ok_or(BackupDoctorError::NotConfigured)?;
        if database_url.trim().is_empty() {
            return Err(BackupDoctorError::NotConfigured);
        }
        if !self.inner.config.script_path.is_file() {
            return Err(BackupDoctorError::EngineUnavailable);
        }
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| BackupDoctorError::RuntimeUnavailable)?;
        let mut active = self
            .inner
            .active_job
            .lock()
            .map_err(|_| BackupDoctorError::Storage)?;
        if active.as_ref().is_some_and(|job| !terminal_status(&job.status)) {
            return Err(BackupDoctorError::AlreadyRunning);
        }

        let now = OffsetDateTime::now_utc().unix_timestamp();
        let id = format!("backup-{now}-{:08x}", rand::random::<u32>());
        let job_dir = self.inner.config.backup_root.join(&id);
        fs::create_dir_all(&job_dir).map_err(|_| BackupDoctorError::Storage)?;
        let job = AdminServerMonitorBackupSnapshot {
            id,
            status: "queued".to_string(),
            source: source.to_string(),
            requested_by: requested_by.trim().to_string(),
            created_at_unix: now,
            ..Default::default()
        };
        write_manifest(&job_dir, &job).map_err(|_| BackupDoctorError::Storage)?;
        *active = Some(job.clone());
        drop(active);

        let doctor = self.clone();
        let spawned_job = job.clone();
        handle.spawn(async move {
            doctor.run_backup(spawned_job).await;
        });
        Ok(job)
    }

    async fn run_backup(&self, mut job: AdminServerMonitorBackupSnapshot) {
        job.status = "running".to_string();
        job.started_at_unix = OffsetDateTime::now_utc().unix_timestamp();
        self.publish_job(&job);

        if let Some(available_mb) = available_disk_mb(&self.inner.config.backup_root).await
            && available_mb < self.inner.config.min_available_mb
        {
            self.finish_failed(
                job,
                format!(
                    "backup uchun disk joyi yetarli emas: {available_mb} MiB mavjud"
                ),
            );
            return;
        }

        let mut command = Command::new("bash");
        command
            .arg(&self.inner.config.script_path)
            .env(
                "MINI_ERP_DATABASE_URL",
                self.inner.config.database_url.as_deref().unwrap_or_default(),
            )
            .env("MINI_ERP_BACKUP_DIR", &self.inner.config.backup_root)
            .env("MINI_ERP_BACKUP_TIMESTAMP", &job.id)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(admin_url) = &self.inner.config.admin_database_url {
            command.env("MINI_ERP_ADMIN_DATABASE_URL", admin_url);
        }
        let output = match timeout(self.inner.config.max_runtime, command.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                self.finish_failed(job, format!("backup engine ishga tushmadi: {error}"));
                return;
            }
            Err(_) => {
                self.finish_failed(job, "backup vaqti chegaradan oshdi".to_string());
                return;
            }
        };
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            self.finish_failed(job, truncate_error(&error));
            return;
        }

        job.status = "verifying".to_string();
        self.publish_job(&job);
        let job_dir = self.inner.config.backup_root.join(&job.id);
        let Some(artifact_path) = preferred_artifact_in(&job_dir) else {
            self.finish_failed(job, "backup dump fayli yaratilmagan".to_string());
            return;
        };
        let checksum_path = artifact_path.clone();
        let checksum = match tokio::task::spawn_blocking(move || sha256_file(&checksum_path)).await {
            Ok(Ok(checksum)) => checksum,
            _ => {
                self.finish_failed(job, "backup checksum tekshiruvi bajarilmadi".to_string());
                return;
            }
        };
        let metadata = match fs::metadata(&artifact_path) {
            Ok(metadata) => metadata,
            Err(_) => {
                self.finish_failed(job, "backup dump metadata o‘qilmadi".to_string());
                return;
            }
        };
        job.status = "ready".to_string();
        job.completed_at_unix = OffsetDateTime::now_utc().unix_timestamp();
        job.size_bytes = metadata.len();
        job.artifact_name = artifact_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string();
        job.checksum_sha256 = checksum;
        job.verified = true;
        job.error.clear();
        self.publish_job(&job);
        self.clear_active(&job.id);
        if self.inner.config.retention_enabled {
            apply_retention(&self.inner.config.backup_root, OffsetDateTime::now_utc());
        }
        tracing::info!(backup_id = %job.id, size_bytes = job.size_bytes, "backup doctor completed backup");
    }

    fn publish_job(&self, job: &AdminServerMonitorBackupSnapshot) {
        let job_dir = self.inner.config.backup_root.join(&job.id);
        if let Err(error) = write_manifest(&job_dir, job) {
            tracing::warn!(%error, backup_id = %job.id, "backup doctor manifest write failed");
        }
        if let Ok(mut active) = self.inner.active_job.lock() {
            *active = Some(job.clone());
        }
    }

    fn finish_failed(&self, mut job: AdminServerMonitorBackupSnapshot, error: String) {
        job.status = "failed".to_string();
        job.completed_at_unix = OffsetDateTime::now_utc().unix_timestamp();
        job.error = truncate_error(&error);
        job.verified = false;
        self.publish_job(&job);
        self.clear_active(&job.id);
        tracing::error!(backup_id = %job.id, error = %job.error, "backup doctor backup failed");
    }

    fn clear_active(&self, id: &str) {
        if let Ok(mut active) = self.inner.active_job.lock()
            && active.as_ref().is_some_and(|job| job.id == id)
        {
            *active = None;
        }
    }

    fn maybe_start_scheduled_backup(&self) {
        let offset_seconds = self.inner.config.utc_offset_minutes.saturating_mul(60);
        let offset = UtcOffset::from_whole_seconds(offset_seconds).unwrap_or(UtcOffset::UTC);
        let now = OffsetDateTime::now_utc();
        let local_now = now.to_offset(offset);
        if (local_now.hour(), local_now.minute())
            < (
                self.inner.config.schedule_hour,
                self.inner.config.schedule_minute,
            )
        {
            return;
        }
        let report = self.report(now);
        let already_ready = report.snapshots.iter().any(|snapshot| {
            snapshot.status == "ready"
                && snapshot.completed_at_unix > 0
                && OffsetDateTime::from_unix_timestamp(snapshot.completed_at_unix)
                    .ok()
                    .is_some_and(|completed| completed.to_offset(offset).date() == local_now.date())
        });
        if already_ready {
            return;
        }
        let recent_attempt = report.snapshots.iter().any(|snapshot| {
            snapshot.source == "automatic"
                && now
                    .unix_timestamp()
                    .saturating_sub(snapshot.created_at_unix)
                    < DEFAULT_AUTO_RETRY_MINUTES * 60
        });
        if recent_attempt {
            return;
        }
        match self.start_backup("automatic", "Backup Doctor".to_string()) {
            Ok(job) => tracing::info!(backup_id = %job.id, "backup doctor scheduled backup"),
            Err(BackupDoctorError::AlreadyRunning) => {}
            Err(error) => tracing::warn!(%error, "backup doctor could not schedule backup"),
        }
    }

    fn reconcile_interrupted_jobs(&self) {
        let root = &self.inner.config.backup_root;
        if !root.is_dir() {
            return;
        }
        let mut manifests = Vec::new();
        collect_manifest_paths(root, &mut manifests);
        for manifest_path in manifests {
            let Ok(body) = fs::read(&manifest_path) else {
                continue;
            };
            let Ok(mut snapshot) =
                serde_json::from_slice::<AdminServerMonitorBackupSnapshot>(&body)
            else {
                continue;
            };
            if terminal_status(&snapshot.status) {
                continue;
            }
            snapshot.status = "failed".to_string();
            snapshot.completed_at_unix = OffsetDateTime::now_utc().unix_timestamp();
            snapshot.error = "server qayta ishga tushgani uchun backup uzildi".to_string();
            snapshot.verified = false;
            if let Some(directory) = manifest_path.parent() {
                let _ = write_manifest(directory, &snapshot);
            }
        }
    }
}

impl Default for BackupDoctor {
    fn default() -> Self {
        Self::from_env()
    }
}

mod catalog;
mod retention;
mod settings;
#[cfg(test)]
mod tests;

use self::catalog::{
    available_disk_mb, collect_manifest_paths, preferred_artifact_in, scan_backup_root,
    sha256_file, terminal_status, truncate_error, write_manifest,
};
use self::retention::apply_retention;
use self::settings::{
    backup_directory, backup_script_path, bool_env, int_env, non_empty_env, parse_clock, uint_env,
};
pub use self::settings::first_existing_backup_directory;
