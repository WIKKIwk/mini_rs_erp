use std::fs;
use std::path::PathBuf;
use std::process::{Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use serde_json;
use thiserror::Error;
use time::{OffsetDateTime, UtcOffset};
use tokio::process::Command;
use tokio::time::timeout;

use crate::core::admin::models::{AdminServerMonitorBackupSnapshot, AdminServerMonitorBackups};

const MANIFEST_NAME: &str = "backup-doctor.json";
const DEFAULT_HEALTH_MAX_AGE_HOURS: i64 = 30;
const DEFAULT_MAX_RUNTIME_MINUTES: u64 = 120;
const DEFAULT_MIN_AVAILABLE_MB: u64 = 1024;
const DEFAULT_AUTO_RETRY_MINUTES: i64 = 60;

#[derive(Debug, Clone)]
pub struct BackupDoctorConfig {
    pub backup_root: PathBuf,
    pub script_path: PathBuf,
    pub restore_script_path: PathBuf,
    pub database_url: Option<String>,
    pub migration_database_url: Option<String>,
    pub admin_database_url: Option<String>,
    pub auto_migrate_after_restore: bool,
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
        let restore_script_path = restore_script_path();
        let (schedule_hour, schedule_minute) = std::env::var("MINI_ERP_BACKUP_TIME")
            .ok()
            .and_then(|value| parse_clock(&value))
            .unwrap_or((2, 0));
        Self {
            backup_root,
            script_path,
            restore_script_path,
            database_url: non_empty_env("MINI_ERP_DATABASE_URL"),
            migration_database_url: non_empty_env("MINI_ERP_MIGRATION_DATABASE_URL"),
            admin_database_url: non_empty_env("MINI_ERP_ADMIN_DATABASE_URL"),
            auto_migrate_after_restore: bool_env("MINI_ERP_AUTO_MIGRATE_AFTER_RESTORE", true),
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
    #[error("backup import is invalid")]
    InvalidImport,
}

#[derive(Debug, Clone)]
pub struct BackupArtifact {
    pub path: PathBuf,
    pub filename: String,
    pub size_bytes: u64,
}

pub struct BackupImportUpload {
    pub job: AdminServerMonitorBackupSnapshot,
    pub path: PathBuf,
}

struct PreRestoreBackup {
    job: AdminServerMonitorBackupSnapshot,
    artifact: BackupArtifact,
}

include!("backup_doctor_impl_parts/part_01.rs");
include!("backup_doctor_impl_parts/part_02.rs");

fn safe_import_name(value: &str) -> Option<String> {
    let name = value.trim().rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned = name
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
        .collect::<String>();
    if cleaned.is_empty() || !cleaned.to_ascii_lowercase().ends_with(".dump") {
        None
    } else {
        Some(cleaned)
    }
}

fn command_failure(prefix: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let details = if stderr.is_empty() { stdout } else { stderr };
    let status = output
        .status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "signal".to_string());
    if details.is_empty() {
        format!("{prefix} muvaffaqiyatsiz tugadi (exit {status})")
    } else {
        format!(
            "{prefix} muvaffaqiyatsiz tugadi (exit {status}): {}",
            truncate_error(&details)
        )
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
pub use self::settings::first_existing_backup_directory;
use self::settings::{
    backup_directory, backup_script_path, bool_env, int_env, non_empty_env, parse_clock,
    restore_script_path, uint_env,
};
