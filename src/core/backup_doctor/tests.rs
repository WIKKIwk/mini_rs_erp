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
        let stale =
            scan_backup_root(root.path(), now + time::Duration::hours(31), 30, None).report;

        assert!(healthy.healthy);
        assert!(!stale.healthy);
    }

    fn write_test_snapshot(
        directory: &Path,
        id: &str,
        status: &str,
        completed: OffsetDateTime,
    ) {
        fs::create_dir_all(directory).expect("snapshot directory");
        let artifact = directory.join("mini_rs_erp.dump");
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
            artifact_name: "mini_rs_erp.dump".to_string(),
            checksum_sha256: checksum,
            verified: status == "ready",
            ..Default::default()
        };
        write_manifest(directory, &snapshot).expect("snapshot manifest");
    }
