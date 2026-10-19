use super::*;
use std::path::Path;

use super::catalog::read_manifest;

#[test]
fn scanner_groups_dump_and_sql_as_one_snapshot() {
    let root = tempfile::tempdir().expect("backup root");
    let directory = root.path().join("20260717_020000");
    fs::create_dir_all(&directory).expect("backup directory");
    fs::write(directory.join("mini.dump"), b"dump").expect("dump");
    fs::write(directory.join("mini.sql"), b"sql").expect("sql");
    let checksum = sha256_file(&directory.join("mini.dump")).expect("checksum");
    fs::write(
        directory.join("SHA256SUMS"),
        format!("{checksum}  mini.dump\n"),
    )
    .expect("checksums");

    let report = scan_backup_root(
        root.path(),
        OffsetDateTime::now_utc(),
        DEFAULT_HEALTH_MAX_AGE_HOURS,
        None,
    )
    .report;

    assert_eq!(report.file_count, 2);
    assert_eq!(report.snapshot_count, 1);
    assert_eq!(report.snapshots.len(), 1);
    assert!(report.snapshots[0].verified);
    assert_eq!(report.snapshots[0].artifact_name, "mini.dump");
    assert!(
        !report.healthy,
        "database-only backups do not prove full recovery"
    );
}

#[test]
fn interrupted_manifest_is_marked_failed_on_startup() {
    let root = tempfile::tempdir().expect("backup root");
    let directory = root.path().join("backup-interrupted");
    let snapshot = AdminServerMonitorBackupSnapshot {
        id: "backup-interrupted".to_string(),
        status: "running".to_string(),
        created_at_unix: OffsetDateTime::now_utc().unix_timestamp(),
        ..Default::default()
    };
    write_manifest(&directory, &snapshot).expect("manifest");
    let _doctor = BackupDoctor::for_test(root.path(), "missing.sh", "postgres://test");

    let recovered = read_manifest(&directory.join(MANIFEST_NAME)).expect("recovered");
    assert_eq!(recovered.status, "failed");
    assert!(recovered.error.contains("qayta ishga"));
}

#[test]
fn retention_prunes_expired_managed_snapshots_but_not_legacy_directories() {
    let root = tempfile::tempdir().expect("backup root");
    let now = OffsetDateTime::now_utc();
    let recent = root.path().join("backup-recent");
    let expired = root.path().join("backup-expired");
    let failed = root.path().join("backup-failed");
    let legacy = root.path().join("legacy-without-manifest");

    write_test_snapshot(
        &recent,
        "backup-recent",
        "ready",
        now - time::Duration::days(2),
    );
    write_test_snapshot(
        &expired,
        "backup-expired",
        "ready",
        now - time::Duration::days(400),
    );
    write_test_snapshot(
        &failed,
        "backup-failed",
        "failed",
        now - time::Duration::days(8),
    );
    fs::create_dir_all(&legacy).expect("legacy directory");
    fs::write(legacy.join("legacy.dump"), b"legacy").expect("legacy dump");

    apply_retention(root.path(), now);

    assert!(recent.is_dir());
    assert!(!expired.exists());
    assert!(!failed.exists());
    assert!(legacy.is_dir());
}

#[test]
fn health_requires_a_recent_verified_snapshot() {
    let root = tempfile::tempdir().expect("backup root");
    let now = OffsetDateTime::now_utc();
    let directory = root.path().join("backup-recent");
    write_test_snapshot(
        &directory,
        "backup-recent",
        "ready",
        now - time::Duration::hours(2),
    );

    let healthy = scan_backup_root(root.path(), now, 30, None).report;
    let stale = scan_backup_root(root.path(), now + time::Duration::hours(31), 30, None).report;

    assert!(healthy.healthy);
    assert!(!stale.healthy);
}

fn write_test_snapshot(directory: &Path, id: &str, status: &str, completed: OffsetDateTime) {
    fs::create_dir_all(directory).expect("snapshot directory");
    let artifact = directory.join("mini_rs_erp.accord-backup.tar.gz");
    fs::write(&artifact, b"verified-backup").expect("snapshot artifact");
    let checksum = sha256_file(&artifact).expect("snapshot checksum");
    let snapshot = AdminServerMonitorBackupSnapshot {
        id: id.to_string(),
        status: status.to_string(),
        source: "automatic".to_string(),
        created_at_unix: completed.unix_timestamp(),
        started_at_unix: completed.unix_timestamp(),
        completed_at_unix: completed.unix_timestamp(),
        size_bytes: fs::metadata(&artifact).expect("metadata").len(),
        artifact_name: "mini_rs_erp.accord-backup.tar.gz".to_string(),
        checksum_sha256: checksum,
        verified: status == "ready",
        recovery_ready: status == "ready",
        ..Default::default()
    };
    write_manifest(directory, &snapshot).expect("snapshot manifest");
}

#[test]
fn download_prefers_complete_bundle_and_rechecks_its_bytes() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("backup-complete");
    write_test_snapshot(
        &directory,
        "backup-complete",
        "ready",
        OffsetDateTime::now_utc(),
    );
    fs::write(directory.join("mini_rs_erp.dump"), b"database-only").unwrap();
    assert!(is_recovery_artifact(
        &preferred_artifact_in(&directory).unwrap()
    ));
    let doctor = BackupDoctor::for_test(root.path(), "missing.sh", "postgres://test");
    assert!(doctor.artifact("backup-complete").is_ok());
    fs::write(
        directory.join("mini_rs_erp.accord-backup.tar.gz"),
        b"tampered-backup",
    )
    .unwrap();
    assert!(matches!(
        doctor.artifact("backup-complete"),
        Err(BackupDoctorError::NotReady)
    ));
    assert!(!doctor.report(OffsetDateTime::now_utc()).healthy);
}

#[test]
fn missing_full_archive_never_falls_back_to_database_only_dump() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("backup-missing");
    write_test_snapshot(
        &directory,
        "backup-missing",
        "ready",
        OffsetDateTime::now_utc(),
    );
    fs::remove_file(directory.join("mini_rs_erp.accord-backup.tar.gz")).unwrap();
    fs::write(directory.join("mini_rs_erp.dump"), b"database-only").unwrap();
    let report = scan_backup_root(root.path(), OffsetDateTime::now_utc(), 30, None).report;
    assert!(!report.healthy);
    assert_eq!(report.snapshots[0].status, "failed");
}

#[test]
fn command_line_full_backup_is_recognized_from_its_verified_summary() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("command-line-backup");
    fs::create_dir(&directory).unwrap();
    let artifact = directory.join("mini.accord-backup.tar.gz");
    fs::write(&artifact, b"full-archive").unwrap();
    let checksum = sha256_file(&artifact).unwrap();
    fs::write(
        directory.join("SHA256SUMS"),
        format!("{checksum}  mini.accord-backup.tar.gz\n"),
    )
    .unwrap();
    fs::write(
        directory.join("mini.accord-backup.tar.gz.json"),
        serde_json::to_vec(&serde_json::json!({
            "format": "accord-full-recovery", "recovery_ready": true, "sha256": checksum
        }))
        .unwrap(),
    )
    .unwrap();
    let report = scan_backup_root(root.path(), OffsetDateTime::now_utc(), 30, None).report;
    assert!(report.healthy);
}

#[test]
fn truncated_artifact_is_not_healthy_or_downloadable() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("backup-truncated");
    write_test_snapshot(
        &directory,
        "backup-truncated",
        "ready",
        OffsetDateTime::now_utc(),
    );
    fs::write(directory.join("mini_rs_erp.accord-backup.tar.gz"), b"x").unwrap();
    let doctor = BackupDoctor::for_test(root.path(), "missing.sh", "postgres://test");
    assert!(!doctor.report(OffsetDateTime::now_utc()).healthy);
    assert!(matches!(
        doctor.artifact("backup-truncated"),
        Err(BackupDoctorError::NotReady)
    ));
}
