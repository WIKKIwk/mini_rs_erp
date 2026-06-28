use super::backup::first_existing_backup_directory;
use super::*;
use crate::core::admin::models::{
    AdminServerMonitorBackups, AdminServerMonitorDatabase, AdminServerMonitorResponse,
    AdminServerMonitorRuntime, AdminServerMonitorServer,
};
use crate::core::admin::monitor_hub::SystemMonitorHub;

#[test]
fn elapsed_ping_ms_never_reports_unknown_zero_for_successful_ping() {
    assert!(elapsed_ping_ms(Instant::now()) >= 1);
}

#[test]
fn first_existing_backup_directory_uses_existing_parent_backup_path() {
    let root = tempfile::tempdir().expect("backup root");
    let missing = root.path().join("mini_rs_erp/backups/mini_rs_erp_db");
    let parent_backup = root.path().join("backups/mini_rs_erp_db");
    fs::create_dir_all(&parent_backup).expect("parent backup dir");

    let selected = first_existing_backup_directory([missing, parent_backup.clone()]);

    assert_eq!(selected, parent_backup);
}

#[test]
fn database_monitor_reports_timeout_as_offline() {
    let database = database_monitor_from_ping_outcome(Instant::now(), DatabasePingOutcome::Timeout);

    assert!(database.configured);
    assert!(!database.reachable);
    assert_eq!(database.status, "offline");
    assert_eq!(database.error, "database ping timed out");
}

#[test]
fn system_monitor_ping_text_returns_app_level_pong() {
    let pong =
        system_monitor_pong_text(r#"{"type":"ping","id":7,"sent_at_ms":12345}"#).expect("pong");
    let payload: serde_json::Value = serde_json::from_str(&pong).expect("pong json");

    assert_eq!(payload["type"], "pong");
    assert_eq!(payload["id"], 7);
    assert_eq!(payload["sent_at_ms"], 12345);
    assert!(payload["server_at_ms"].as_i64().unwrap_or(0) > 0);
}

#[tokio::test]
async fn system_monitor_hub_fans_out_latest_snapshot_to_all_subscribers() {
    let hub = SystemMonitorHub::new();
    assert!(hub.mark_started());
    assert!(!hub.mark_started());

    let mut first = hub.subscribe();
    let mut second = hub.subscribe();
    let snapshot = AdminServerMonitorResponse {
        server: AdminServerMonitorServer {
            status: "running".to_string(),
            ..Default::default()
        },
        database: AdminServerMonitorDatabase {
            reachable: true,
            status: "online".to_string(),
            ..Default::default()
        },
        backups: AdminServerMonitorBackups {
            exists: true,
            ..Default::default()
        },
        runtime: AdminServerMonitorRuntime::default(),
    };

    hub.publish(snapshot.clone());

    first.changed().await.expect("first subscriber update");
    second.changed().await.expect("second subscriber update");
    assert_eq!(first.borrow().as_ref(), Some(&snapshot));
    assert_eq!(second.borrow().as_ref(), Some(&snapshot));
}
