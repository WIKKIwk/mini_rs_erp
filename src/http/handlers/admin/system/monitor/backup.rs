use crate::core::admin::models::{AdminServerMonitorBackupFile, AdminServerMonitorBackups};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;

pub(super) fn scan_backup_directory(now: OffsetDateTime) -> AdminServerMonitorBackups {
    let directory = backup_directory();
    let mut snapshot = AdminServerMonitorBackups {
        directory: directory.display().to_string(),
        exists: directory.is_dir(),
        ..Default::default()
    };
    if !snapshot.exists {
        snapshot.error = "backup directory not found".to_string();
        return snapshot;
    }
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) => {
            snapshot.error = error.to_string();
            return snapshot;
        }
    };
    let mut files: Vec<(SystemTime, PathBuf, u64)> = Vec::new();
    collect_backup_files(entries, &mut snapshot.file_count, &mut files);
    files.sort_by_key(|file| std::cmp::Reverse(file.0));
    snapshot.files = files
        .into_iter()
        .map(|(modified, path, size_bytes)| backup_file_snapshot(now, modified, path, size_bytes))
        .collect();
    snapshot.latest = snapshot.files.first().cloned();
    snapshot
}

fn collect_backup_files(
    entries: fs::ReadDir,
    file_count: &mut usize,
    files: &mut Vec<(SystemTime, PathBuf, u64)>,
) {
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            if let Ok(child_entries) = fs::read_dir(path) {
                collect_backup_files(child_entries, file_count, files);
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        if !is_backup_data_file(&path) {
            continue;
        }
        *file_count += 1;
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        files.push((modified, path, metadata.len()));
    }
}

fn is_backup_data_file(path: &std::path::Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    name.ends_with(".dump") || name.ends_with(".sql") || name.ends_with(".sql.gz")
}

fn backup_file_snapshot(
    now: OffsetDateTime,
    modified: SystemTime,
    path: PathBuf,
    size_bytes: u64,
) -> AdminServerMonitorBackupFile {
    let modified_at_unix = system_time_to_unix(modified);
    let age_seconds = now.unix_timestamp().saturating_sub(modified_at_unix).max(0);
    AdminServerMonitorBackupFile {
        name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string(),
        path: path.display().to_string(),
        size_bytes,
        modified_at_unix,
        age_seconds,
    }
}

fn backup_directory() -> PathBuf {
    if let Some(path) = std::env::var("MINI_ERP_BACKUP_DIR")
        .ok()
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return path;
    }
    first_existing_backup_directory([
        PathBuf::from("backups/mini_rs_erp_db"),
        PathBuf::from("../backups/mini_rs_erp_db"),
    ])
}

pub(super) fn first_existing_backup_directory<const N: usize>(candidates: [PathBuf; N]) -> PathBuf {
    let fallback = candidates
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("backups/mini_rs_erp_db"));
    candidates
        .into_iter()
        .find(|candidate| candidate.is_dir())
        .unwrap_or(fallback)
}

fn system_time_to_unix(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}
