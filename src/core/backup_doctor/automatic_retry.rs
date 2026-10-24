use time::{OffsetDateTime, UtcOffset};

use crate::core::admin::models::AdminServerMonitorBackupSnapshot;

const MAX_DAILY_ATTEMPTS: usize = 3;
const RETRY_BASE_SECONDS: i64 = 60 * 60;

pub(super) fn scheduled_backup_due(
    snapshots: &[AdminServerMonitorBackupSnapshot],
    now: OffsetDateTime,
    offset: UtcOffset,
    schedule_hour: u8,
    schedule_minute: u8,
) -> bool {
    let local_now = now.to_offset(offset);
    if (local_now.hour(), local_now.minute()) < (schedule_hour, schedule_minute) {
        return false;
    }
    let is_today = |timestamp| {
        OffsetDateTime::from_unix_timestamp(timestamp)
            .ok()
            .is_some_and(|time| time.to_offset(offset).date() == local_now.date())
    };
    if snapshots.iter().any(|snapshot| {
        snapshot.status == "ready"
            && snapshot.recovery_ready
            && snapshot.completed_at_unix > 0
            && is_today(snapshot.completed_at_unix)
    }) {
        return false;
    }

    // Persisted manifests keep the retry budget intact after a server restart.
    let attempts_today = snapshots
        .iter()
        .filter(|snapshot| snapshot.source == "automatic" && is_today(snapshot.created_at_unix))
        .count();
    if attempts_today >= MAX_DAILY_ATTEMPTS {
        return false;
    }
    let last_attempt = snapshots
        .iter()
        .filter(|snapshot| snapshot.source == "automatic")
        .map(|snapshot| {
            snapshot
                .created_at_unix
                .max(snapshot.started_at_unix)
                .max(snapshot.completed_at_unix)
        })
        .max();
    // Wait one hour after the first failure and two after the second. Even a
    // new local day must respect the global one-hour cooldown. A backwards
    // clock never makes a future-dated attempt immediately eligible again.
    let delay = RETRY_BASE_SECONDS * (attempts_today as i64).max(1);
    last_attempt.is_none_or(|last| now.unix_timestamp().saturating_sub(last) >= delay)
}

#[cfg(test)]
mod tests {
    use super::super::{
        BackupDoctor, BackupDoctorError, DoctorPolicy, SystemDoctor, write_manifest,
    };
    use super::*;
    use crate::core::admin::models::{AdminServerMonitorDatabase, AdminServerMonitorRuntime};
    use std::sync::Arc;
    use time::{Date, Month};

    fn offset() -> UtcOffset {
        UtcOffset::from_hms(5, 0, 0).unwrap()
    }

    fn at(day: u8, hour: u8, minute: u8) -> OffsetDateTime {
        Date::from_calendar_date(2026, Month::October, day)
            .unwrap()
            .with_hms(hour, minute, 0)
            .unwrap()
            .assume_offset(offset())
    }

    fn failed_at(day: u8, hour: u8, minute: u8) -> AdminServerMonitorBackupSnapshot {
        let timestamp = at(day, hour, minute).unix_timestamp();
        AdminServerMonitorBackupSnapshot {
            source: "automatic".to_string(),
            status: "failed".to_string(),
            created_at_unix: timestamp,
            started_at_unix: timestamp,
            completed_at_unix: timestamp,
            ..Default::default()
        }
    }

    fn due(snapshots: &[AdminServerMonitorBackupSnapshot], now: OffsetDateTime) -> bool {
        scheduled_backup_due(snapshots, now, offset(), 2, 0)
    }

    #[test]
    fn first_attempt_waits_until_local_schedule() {
        assert!(!due(&[], at(24, 1, 59)));
        assert!(due(&[], at(24, 2, 0)));
    }

    #[test]
    fn failures_back_off_from_completion_for_one_then_two_hours() {
        let mut first = failed_at(24, 2, 0);
        first.completed_at_unix = at(24, 2, 30).unix_timestamp();
        assert!(!due(&[first.clone()], at(24, 3, 29)));
        assert!(due(&[first.clone()], at(24, 3, 30)));
        let attempts = [first, failed_at(24, 3, 30)];
        assert!(!due(&attempts, at(24, 5, 29)));
        assert!(due(&attempts, at(24, 5, 30)));
    }

    #[test]
    fn third_failure_exhausts_local_daily_budget() {
        let attempts = [
            failed_at(24, 2, 0),
            failed_at(24, 3, 0),
            failed_at(24, 5, 0),
        ];
        assert!(!due(&attempts, at(24, 23, 59)));
        assert!(due(&attempts, at(25, 2, 0)));
    }

    #[test]
    fn local_midnight_resets_budget_but_keeps_global_cooldown() {
        let attempts = [
            failed_at(24, 20, 0),
            failed_at(24, 21, 0),
            failed_at(24, 23, 45),
        ];
        assert!(!scheduled_backup_due(
            &attempts,
            at(25, 0, 44),
            offset(),
            0,
            0
        ));
        assert!(scheduled_backup_due(
            &attempts,
            at(25, 0, 45),
            offset(),
            0,
            0
        ));
    }

    #[test]
    fn backwards_clock_does_not_bypass_cooldown() {
        assert!(!due(&[failed_at(25, 2, 0)], at(24, 6, 0)));
    }

    #[test]
    fn manual_failures_do_not_use_automatic_retry_budget() {
        let attempts = (0..4)
            .map(|_| AdminServerMonitorBackupSnapshot {
                source: "manual".to_string(),
                ..failed_at(24, 5, 0)
            })
            .collect::<Vec<_>>();
        assert!(due(&attempts, at(24, 5, 0)));
    }

    #[test]
    fn successful_full_recovery_snapshot_skips_the_day() {
        let mut snapshot = failed_at(24, 5, 0);
        snapshot.source = "manual".to_string();
        snapshot.status = "ready".to_string();
        snapshot.recovery_ready = true;
        assert!(!due(&[snapshot.clone()], at(24, 8, 0)));
        snapshot.recovery_ready = false;
        assert!(due(&[snapshot], at(24, 8, 0)));
    }

    #[test]
    fn persisted_retry_budget_survives_doctor_recreation() {
        let root = tempfile::tempdir().unwrap();
        for (index, hour) in [2, 3, 5].into_iter().enumerate() {
            let snapshot = AdminServerMonitorBackupSnapshot {
                id: format!("backup-{index}"),
                ..failed_at(24, hour, 0)
            };
            write_manifest(&root.path().join(&snapshot.id), &snapshot).unwrap();
        }
        let doctor = BackupDoctor::for_test(root.path(), "missing.sh", "postgres://test");
        assert!(!due(&doctor.report(at(24, 8, 0)).snapshots, at(24, 8, 0)));
        drop(doctor);
        let recreated = BackupDoctor::for_test(root.path(), "missing.sh", "postgres://test");
        assert!(!due(
            &recreated.report(at(24, 8, 0)).snapshots,
            at(24, 8, 0)
        ));
        assert!(due(&recreated.report(at(25, 2, 0)).snapshots, at(25, 2, 0)));
    }

    fn protect(doctor: &BackupDoctor) {
        let database = AdminServerMonitorDatabase {
            configured: true,
            reachable: true,
            ..Default::default()
        };
        let runtime = AdminServerMonitorRuntime {
            memory_total_mb: 10_000,
            memory_percent: 95,
            disk_total_mb: 100_000,
            disk_available_mb: 50_000,
            ..Default::default()
        };
        // Accessor clones share the monitor state used for job admission.
        doctor
            .system_doctor()
            .observe(&database, &runtime, at(24, 6, 0).unix_timestamp());
        assert!(doctor.system_doctor().protected());
    }

    #[test]
    fn protection_defers_automatic_and_rejects_manual_import_without_reserving_jobs() {
        let root = tempfile::tempdir().unwrap();
        let backups = root.path().join("backups");
        let mut doctor = BackupDoctor::for_test(&backups, "missing.sh", "postgres://test");
        Arc::get_mut(&mut doctor.inner).unwrap().system_doctor = SystemDoctor::new(DoctorPolicy {
            failure_samples: 1,
            ..Default::default()
        });
        protect(&doctor);

        assert!(matches!(
            doctor.start_manual_backup("admin"),
            Err(BackupDoctorError::ResourcePressure)
        ));
        assert!(matches!(
            doctor.start_backup("automatic", "Doctor".to_string()),
            Err(BackupDoctorError::ResourcePressure)
        ));
        assert!(matches!(
            doctor.prepare_import("admin", "mini.dump"),
            Err(BackupDoctorError::ResourcePressure)
        ));
        doctor.maybe_start_scheduled_backup();
        assert!(doctor.inner.active_job.lock().unwrap().is_none());
        assert!(
            !backups.exists(),
            "protection must not consume storage or create failed manifests"
        );
    }

    #[tokio::test]
    async fn protection_preserves_an_import_that_was_already_reserved() {
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("backup.sh");
        let restore_script = root.path().join("restore.sh");
        std::fs::write(&script, b"exit 0\n").unwrap();
        std::fs::write(&restore_script, b"exit 0\n").unwrap();
        let mut doctor = BackupDoctor::for_test_with_restore(
            root.path().join("backups"),
            &script,
            &restore_script,
            "postgres://test",
        );
        Arc::get_mut(&mut doctor.inner).unwrap().system_doctor = SystemDoctor::new(DoctorPolicy {
            failure_samples: 1,
            ..Default::default()
        });
        let upload = doctor.prepare_import("admin", "mini.dump").unwrap();
        protect(&doctor);
        assert_eq!(
            doctor.inner.active_job.lock().unwrap().as_ref().unwrap().id,
            upload.job.id
        );
        assert!(matches!(
            doctor.start_manual_backup("admin"),
            Err(BackupDoctorError::ResourcePressure)
        ));
        assert_eq!(
            doctor.report(at(24, 6, 0)).active_job.unwrap().id,
            upload.job.id
        );
    }
}
