use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::process::Command;

use crate::core::admin::models::{
    AdminServerMonitorBackupFile, AdminServerMonitorBackupSnapshot, AdminServerMonitorBackups,
};

use super::{BackupArtifact, MANIFEST_NAME};

pub(super) struct BackupScan {
    pub(super) report: AdminServerMonitorBackups,
    pub(super) artifacts: HashMap<String, BackupArtifact>,
}

#[derive(Clone)]
struct BackupDataFile {
    modified: SystemTime,
    path: PathBuf,
    size_bytes: u64,
}

pub(super) fn scan_backup_root(
    root: &Path,
    now: OffsetDateTime,
    health_max_age_hours: i64,
    active_job: Option<AdminServerMonitorBackupSnapshot>,
) -> BackupScan {
    let mut report = AdminServerMonitorBackups {
        directory: root.display().to_string(),
        exists: root.is_dir(),
        ..Default::default()
    };
    if !report.exists {
        report.error = "backup directory not found".to_string();
        report.active_job = active_job;
        return BackupScan {
            report,
            artifacts: HashMap::new(),
        };
    }
    let mut data_files = Vec::new();
    let mut manifest_paths = Vec::new();
    collect_backup_entries(root, &mut data_files, &mut manifest_paths);
    data_files.sort_by_key(|file| std::cmp::Reverse(file.modified));
    report.file_count = data_files.len();
    report.files = data_files
        .iter()
        .map(|file| backup_file_snapshot(now, file))
        .collect();
    report.latest = report.files.first().cloned();

    let mut by_directory = BTreeMap::<PathBuf, Vec<BackupDataFile>>::new();
    for file in &data_files {
        if let Some(parent) = file.path.parent() {
            by_directory
                .entry(parent.to_path_buf())
                .or_default()
                .push(file.clone());
        }
    }
    for manifest in manifest_paths {
        if let Some(parent) = manifest.parent() {
            by_directory.entry(parent.to_path_buf()).or_default();
        }
    }

    let mut artifacts = HashMap::new();
    let mut snapshots = Vec::new();
    for (directory, mut files) in by_directory {
        files.sort_by_key(|file| std::cmp::Reverse(file.modified));
        let manifest = read_manifest(&directory.join(MANIFEST_NAME));
        let mut snapshot = manifest.unwrap_or_else(|| legacy_snapshot(&directory, &files));
        if snapshot.id.trim().is_empty() {
            snapshot.id = legacy_snapshot_id(&directory);
        }
        if let Some(artifact_file) = select_artifact(&files, &snapshot.artifact_name) {
            if snapshot.artifact_name.is_empty() {
                snapshot.artifact_name = artifact_file
                    .path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_string();
            }
            if snapshot.size_bytes == 0 {
                snapshot.size_bytes = artifact_file.size_bytes;
            }
            if snapshot.status == "ready" && snapshot.verified {
                artifacts.insert(
                    snapshot.id.clone(),
                    BackupArtifact {
                        path: artifact_file.path.clone(),
                        filename: snapshot.artifact_name.clone(),
                        size_bytes: artifact_file.size_bytes,
                    },
                );
            }
        } else if snapshot.status == "ready" {
            snapshot.status = "failed".to_string();
            snapshot.verified = false;
            snapshot.error = "backup artifact topilmadi".to_string();
        }
        snapshots.push(snapshot);
    }
    if let Some(active) = active_job.clone() {
        if let Some(existing) = snapshots.iter_mut().find(|item| item.id == active.id) {
            *existing = active.clone();
        } else {
            snapshots.push(active);
        }
    }
    snapshots.sort_by_key(|snapshot| {
        std::cmp::Reverse(
            snapshot
                .completed_at_unix
                .max(snapshot.started_at_unix)
                .max(snapshot.created_at_unix),
        )
    });
    report.snapshot_count = snapshots
        .iter()
        .filter(|snapshot| snapshot.status == "ready" && snapshot.verified)
        .count();
    report.latest_snapshot = snapshots
        .iter()
        .find(|snapshot| snapshot.status == "ready" && snapshot.verified)
        .cloned();
    report.healthy = report.latest_snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.verified
            && snapshot.completed_at_unix > 0
            && now
                .unix_timestamp()
                .saturating_sub(snapshot.completed_at_unix)
                <= health_max_age_hours.max(1) * 60 * 60
    });
    report.snapshots = snapshots;
    report.active_job = active_job;
    BackupScan { report, artifacts }
}

fn collect_backup_entries(
    directory: &Path,
    files: &mut Vec<BackupDataFile>,
    manifests: &mut Vec<PathBuf>,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_backup_entries(&path, files, manifests);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if path.file_name().and_then(|value| value.to_str()) == Some(MANIFEST_NAME) {
            manifests.push(path);
            continue;
        }
        if !is_backup_data_file(&path) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        files.push(BackupDataFile {
            modified,
            path,
            size_bytes: metadata.len(),
        });
    }
}

pub(super) fn collect_manifest_paths(directory: &Path, manifests: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_manifest_paths(&path, manifests);
        } else if file_type.is_file()
            && path.file_name().and_then(|value| value.to_str()) == Some(MANIFEST_NAME)
        {
            manifests.push(path);
        }
    }
}

fn legacy_snapshot(directory: &Path, files: &[BackupDataFile]) -> AdminServerMonitorBackupSnapshot {
    let artifact = select_artifact(files, "");
    let completed_at_unix = files
        .iter()
        .map(|file| system_time_to_unix(file.modified))
        .max()
        .unwrap_or(0);
    let artifact_name = artifact
        .and_then(|file| file.path.file_name())
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    let checksum = read_checksum(directory, &artifact_name).unwrap_or_default();
    AdminServerMonitorBackupSnapshot {
        id: legacy_snapshot_id(directory),
        status: "ready".to_string(),
        source: "legacy".to_string(),
        requested_by: String::new(),
        created_at_unix: completed_at_unix,
        started_at_unix: completed_at_unix,
        completed_at_unix,
        size_bytes: artifact.map(|file| file.size_bytes).unwrap_or(0),
        artifact_name,
        checksum_sha256: checksum.clone(),
        verified: !checksum.is_empty(),
        error: String::new(),
    }
}

fn legacy_snapshot_id(directory: &Path) -> String {
    let digest = Sha256::digest(directory.to_string_lossy().as_bytes());
    let encoded = format!("{digest:x}");
    format!("legacy-{}", &encoded[..24])
}

fn select_artifact<'a>(
    files: &'a [BackupDataFile],
    preferred_name: &str,
) -> Option<&'a BackupDataFile> {
    if !preferred_name.trim().is_empty()
        && let Some(file) = files.iter().find(|file| {
            file.path.file_name().and_then(|value| value.to_str()) == Some(preferred_name)
        })
    {
        return Some(file);
    }
    files
        .iter()
        .find(|file| extension_rank(&file.path) == 0)
        .or_else(|| files.iter().min_by_key(|file| extension_rank(&file.path)))
}

fn extension_rank(path: &Path) -> u8 {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".dump") {
        0
    } else if name.ends_with(".sql.gz") {
        1
    } else {
        2
    }
}

fn read_checksum(directory: &Path, artifact_name: &str) -> Option<String> {
    let body = fs::read_to_string(directory.join("SHA256SUMS")).ok()?;
    body.lines().find_map(|line| {
        let (checksum, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*');
        (name == artifact_name && checksum.len() == 64).then(|| checksum.to_string())
    })
}

pub(super) fn preferred_artifact_in(directory: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(directory).ok()?;
    let mut candidates = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_backup_data_file(path))
        .collect::<Vec<_>>();
    candidates.sort_by_key(|path| extension_rank(path));
    candidates.into_iter().next()
}

fn backup_file_snapshot(
    now: OffsetDateTime,
    file: &BackupDataFile,
) -> AdminServerMonitorBackupFile {
    let modified_at_unix = system_time_to_unix(file.modified);
    AdminServerMonitorBackupFile {
        name: file
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_string(),
        path: file.path.display().to_string(),
        size_bytes: file.size_bytes,
        modified_at_unix,
        age_seconds: now.unix_timestamp().saturating_sub(modified_at_unix).max(0),
    }
}

fn is_backup_data_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    name.ends_with(".dump") || name.ends_with(".sql") || name.ends_with(".sql.gz")
}

pub(super) fn write_manifest(
    directory: &Path,
    snapshot: &AdminServerMonitorBackupSnapshot,
) -> Result<(), std::io::Error> {
    fs::create_dir_all(directory)?;
    let path = directory.join(MANIFEST_NAME);
    let temporary = directory.join(format!("{MANIFEST_NAME}.tmp"));
    let body = serde_json::to_vec_pretty(snapshot).map_err(std::io::Error::other)?;
    fs::write(&temporary, body)?;
    fs::rename(temporary, path)
}

pub(super) fn read_manifest(path: &Path) -> Option<AdminServerMonitorBackupSnapshot> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

pub(super) fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(super) async fn available_disk_mb(path: &Path) -> Option<u64> {
    let output = Command::new("df")
        .arg("-Pk")
        .arg(path)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let body = String::from_utf8_lossy(&output.stdout);
    let line = body.lines().rev().find(|line| !line.trim().is_empty())?;
    let fields = line.split_whitespace().collect::<Vec<_>>();
    let available_kb = fields
        .get(fields.len().checked_sub(3)?)?
        .parse::<u64>()
        .ok()?;
    Some(available_kb / 1024)
}

pub(super) fn terminal_status(status: &str) -> bool {
    matches!(status, "ready" | "failed" | "cancelled")
}

pub(super) fn truncate_error(error: &str) -> String {
    let cleaned = error.trim();
    if cleaned.is_empty() {
        return "backup bajarilmadi".to_string();
    }
    cleaned.chars().take(500).collect()
}

fn system_time_to_unix(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}
