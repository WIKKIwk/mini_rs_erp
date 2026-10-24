use super::*;

#[test]
fn wifi_report_survives_resource_sampling_without_exposing_restore_tokens_in_journal() {
    let doctor = SystemDoctor::default();
    doctor.publish_wifi(wifi::WifiRecoveryReport {
        enabled: true,
        status: "verifying".to_string(),
        current_ssid: Some("Open guest Wi-Fi".to_string()),
        checked_at_unix: 100,
        ..Default::default()
    });
    let report = doctor.observe(&database(true), &runtime(50, 4000), 101);
    assert_eq!(report.wifi.status, "verifying");
    assert_eq!(
        report.wifi.current_ssid.as_deref(),
        Some("Open guest Wi-Fi")
    );
    let event = report
        .events
        .iter()
        .find(|event| event.rule_id == "wifi")
        .unwrap();
    assert_eq!(event.at_unix, 100);
    assert_eq!(event.message, "Wi-Fi recovery status: verifying");
    assert!(!report.protected);
}

fn database(reachable: bool) -> AdminServerMonitorDatabase {
    AdminServerMonitorDatabase {
        configured: true,
        reachable,
        ..AdminServerMonitorDatabase::default()
    }
}

fn runtime(memory_percent: i64, disk_available_mb: i64) -> AdminServerMonitorRuntime {
    AdminServerMonitorRuntime {
        memory_total_mb: 8192,
        memory_percent,
        disk_total_mb: 10_000,
        disk_available_mb,
        ..AdminServerMonitorRuntime::default()
    }
}

fn check<'a>(report: &'a DoctorReport, id: &str) -> &'a DoctorCheck {
    report.checks.iter().find(|check| check.id == id).unwrap()
}

fn observe_times(
    doctor: &SystemDoctor,
    database: &AdminServerMonitorDatabase,
    runtime: &AdminServerMonitorRuntime,
    times: i64,
) -> DoctorReport {
    let mut report = doctor.snapshot();
    for time in 1..=times {
        report = doctor.observe(database, runtime, time);
    }
    report
}

#[test]
fn transient_database_failure_does_not_confirm_an_incident() {
    let doctor = SystemDoctor::default();
    assert_eq!(doctor.snapshot().status, "starting");
    let resources = runtime(50, 4000);
    let report = observe_times(&doctor, &database(false), &resources, 2);
    assert_eq!(report.status, "warning");
    assert_eq!(check(&report, "database").consecutive_failures, 2);
    assert!(!check(&report, "database").critical);
    assert!(!report.protected);

    let report = doctor.observe(&database(true), &resources, 3);
    assert_eq!(report.status, "healthy");
    assert_eq!(check(&report, "database").consecutive_failures, 0);
}

#[test]
fn confirmed_database_failure_requires_repeated_recovery_without_pausing_backups() {
    let doctor = SystemDoctor::default();
    let resources = runtime(50, 4000);
    let report = observe_times(&doctor, &database(false), &resources, 3);
    assert_eq!(report.status, "critical");
    assert!(check(&report, "database").critical);
    assert!(!doctor.protected());

    let report = doctor.observe(&database(true), &resources, 4);
    assert_eq!(report.status, "recovering");
    assert!(check(&report, "database").critical);
    let report = doctor.observe(&database(true), &resources, 5);
    assert_eq!(report.status, "healthy");
    assert!(!check(&report, "database").critical);
}

#[test]
fn memory_hysteresis_keeps_protection_until_consecutive_recovery_samples() {
    let doctor = SystemDoctor::default();
    let db = database(true);
    let report = observe_times(&doctor, &db, &runtime(90, 4000), 2);
    assert_eq!(check(&report, "memory").status, "warning");
    assert!(!report.protected);
    let report = doctor.observe(&db, &runtime(90, 4000), 3);
    assert_eq!(report.status, "critical");
    assert!(doctor.protected());

    let report = doctor.observe(&db, &runtime(85, 4000), 4);
    assert_eq!(check(&report, "memory").status, "critical");
    assert!(report.protected);
    let report = doctor.observe(&db, &runtime(80, 4000), 5);
    assert_eq!(report.status, "recovering");
    assert!(report.protected);
    let report = doctor.observe(&db, &runtime(85, 4000), 6);
    assert_eq!(check(&report, "memory").status, "critical");
    assert_eq!(check(&report, "memory").consecutive_successes, 0);

    assert!(doctor.observe(&db, &runtime(80, 4000), 7).protected);
    let report = doctor.observe(&db, &runtime(80, 4000), 8);
    assert_eq!(report.status, "healthy");
    assert!(!report.protected);
}

#[test]
fn disk_recovery_uses_free_space_threshold_and_hysteresis() {
    let doctor = SystemDoctor::default();
    let db = database(true);
    let report = observe_times(&doctor, &db, &runtime(50, 1024), 3);
    assert_eq!(check(&report, "disk").status, "critical");
    assert!(report.protected);

    let report = doctor.observe(&db, &runtime(50, 1500), 4);
    assert_eq!(check(&report, "disk").status, "critical");
    assert!(report.protected);
    let report = doctor.observe(&db, &runtime(50, 1536), 5);
    assert_eq!(check(&report, "disk").status, "recovering");
    assert!(report.protected);
    let report = doctor.observe(&db, &runtime(50, 1536), 6);
    assert_eq!(report.status, "healthy");
    assert!(!report.protected);
}

#[test]
fn unavailable_samples_are_unknown_and_break_failure_confirmation() {
    let doctor = SystemDoctor::default();
    let report = doctor.observe(
        &AdminServerMonitorDatabase::default(),
        &AdminServerMonitorRuntime::default(),
        1,
    );
    assert_eq!(report.status, "unknown");
    assert!(report.checks.iter().all(|check| check.status == "unknown"));

    observe_times(&doctor, &database(true), &runtime(95, 100), 2);
    doctor.observe(&database(true), &AdminServerMonitorRuntime::default(), 4);
    let report = doctor.observe(&database(true), &runtime(95, 100), 5);
    assert!(!report.protected);
    assert_eq!(check(&report, "memory").consecutive_failures, 1);
    assert_eq!(check(&report, "disk").consecutive_failures, 1);
}

#[test]
fn unknown_samples_cannot_resolve_incidents_or_continue_partial_recovery() {
    let doctor = SystemDoctor::default();
    observe_times(&doctor, &database(false), &runtime(95, 100), 3);
    let report = doctor.observe(&database(true), &runtime(50, 4000), 4);
    assert_eq!(report.status, "recovering");

    let report = doctor.observe(
        &AdminServerMonitorDatabase::default(),
        &AdminServerMonitorRuntime::default(),
        5,
    );
    assert_eq!(report.status, "critical");
    assert!(report.protected);
    assert!(report.checks.iter().all(|check| {
        check.status == "unknown" && check.critical && check.consecutive_successes == 0
    }));
    let report = doctor.observe(&database(true), &runtime(50, 4000), 6);
    assert_eq!(report.status, "recovering");
    assert!(report.protected);
    let report = doctor.observe(&database(true), &runtime(50, 4000), 7);
    assert_eq!(report.status, "healthy");
    assert!(!report.protected);
}

#[test]
fn memory_and_disk_protection_recover_independently() {
    let doctor = SystemDoctor::default();
    observe_times(&doctor, &database(false), &runtime(95, 100), 3);
    doctor.observe(&database(false), &runtime(50, 100), 4);
    let report = doctor.observe(&database(false), &runtime(50, 100), 5);
    assert!(!check(&report, "memory").critical);
    assert!(check(&report, "disk").critical);
    assert!(report.protected);

    doctor.observe(&database(false), &runtime(50, 4000), 6);
    let report = doctor.observe(&database(false), &runtime(50, 4000), 7);
    assert_eq!(report.status, "critical");
    assert!(check(&report, "database").critical);
    assert!(!report.protected);
}

#[test]
fn transition_journal_is_bounded_and_unchanged_samples_do_not_spam_it() {
    let doctor = SystemDoctor::default();
    let resources = runtime(50, 4000);
    let report = doctor.observe(&database(true), &resources, 1);
    assert_eq!(report.events.len(), 3);
    let report = observe_times(&doctor, &database(true), &resources, 100);
    assert_eq!(report.events.len(), 3);

    let mut db = database(true);
    db.error = "postgresql://secret_user:secret_password@db/private".to_string();
    for now in 101..181 {
        db.reachable = now % 2 == 0;
        doctor.observe(&db, &resources, now);
    }
    let report = doctor.snapshot();
    assert_eq!(report.events.len(), MAX_EVENTS);
    assert!(report.events.windows(2).all(|events| {
        events[0].at_unix < events[1].at_unix && events[0].to_status == events[1].from_status
    }));
    assert!(!serde_json::to_string(&report).unwrap().contains("secret"));
}

#[test]
fn custom_policy_is_applied_and_invalid_thresholds_are_normalized() {
    let doctor = SystemDoctor::new(DoctorPolicy {
        failure_samples: 1,
        recovery_samples: 1,
        memory_critical_percent: 70,
        memory_recovery_percent: 60,
        disk_critical_available_mb: 500,
        disk_recovery_available_mb: 750,
    });
    let report = doctor.observe(&database(true), &runtime(70, 500), 1);
    assert!(report.protected);
    let report = doctor.observe(&database(true), &runtime(60, 750), 2);
    assert!(!report.protected);

    let policy = DoctorPolicy {
        failure_samples: 0,
        recovery_samples: u32::MAX,
        memory_critical_percent: 200,
        memory_recovery_percent: 200,
        disk_critical_available_mb: i64::MAX,
        disk_recovery_available_mb: -1,
    }
    .normalized();
    assert_eq!(policy.failure_samples, 1);
    assert_eq!(policy.recovery_samples, 60);
    assert_eq!(policy.memory_critical_percent, 100);
    assert_eq!(policy.memory_recovery_percent, 99);
    assert_eq!(policy.disk_critical_available_mb, i64::MAX - 1);
    assert_eq!(policy.disk_recovery_available_mb, i64::MAX);
}

#[test]
fn clones_share_state_and_reports_accept_missing_serialized_fields() {
    let doctor = SystemDoctor::default();
    let clone = doctor.clone();
    let report = observe_times(&doctor, &database(true), &runtime(95, 4000), 3);
    assert_eq!(clone.snapshot(), report);
    assert!(clone.protected());

    let empty: DoctorReport = serde_json::from_str("{}").unwrap();
    assert_eq!(empty, DoctorReport::default());
    let restored: DoctorReport =
        serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
    assert_eq!(restored, report);
}
