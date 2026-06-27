use super::*;
use crate::core::admin::models::{
    AdminServerMonitorBackupFile, AdminServerMonitorBackups, AdminServerMonitorDatabase,
    AdminServerMonitorResponse, AdminServerMonitorRuntime, AdminServerMonitorServer,
};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;
use tokio::time::{Duration, timeout};

const DATABASE_PING_TIMEOUT: Duration = Duration::from_secs(2);
const LIVE_SNAPSHOT_INTERVAL: Duration = Duration::from_secs(2);
const LIVE_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const LIVE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);

pub async fn system_monitor(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AdminServerMonitorResponse>, AdminError> {
    authorize_capability(&state, &headers, Capability::AdminAccess).await?;
    Ok(Json(system_monitor_report(&state).await))
}

pub async fn system_monitor_live(
    State(state): State<AppState>,
    Query(query): Query<SystemMonitorLiveQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, AdminError> {
    let principal = authenticated_principal_for_live(&state, &headers, &query.token).await?;
    require_capability(&state, &principal, Capability::AdminAccess).await?;
    Ok(ws
        .on_upgrade(move |socket| system_monitor_live_socket(state, socket))
        .into_response())
}

#[derive(Default, Deserialize)]
pub struct SystemMonitorLiveQuery {
    #[serde(default)]
    token: String,
}

async fn system_monitor_report(state: &AppState) -> AdminServerMonitorResponse {
    let now = OffsetDateTime::now_utc();
    let uptime_seconds = state.started_at.elapsed().as_secs().min(i64::MAX as u64) as i64;
    let server = AdminServerMonitorServer {
        bind_addr: state.config.bind_addr.to_string(),
        started_at_unix: state.started_at_unix,
        uptime_seconds,
        status: "running".to_string(),
    };
    let database = match &state.mini_engine {
        Some(engine) => {
            let started = Instant::now();
            let outcome = match timeout(DATABASE_PING_TIMEOUT, engine.ping()).await {
                Ok(Ok(())) => DatabasePingOutcome::Online,
                Ok(Err(error)) => DatabasePingOutcome::Error(error.to_string()),
                Err(_) => DatabasePingOutcome::Timeout,
            };
            database_monitor_from_ping_outcome(started, outcome)
        }
        None => AdminServerMonitorDatabase {
            configured: false,
            reachable: false,
            status: "unavailable".to_string(),
            ping_ms: 0,
            error: "mini engine store is not configured".to_string(),
        },
    };
    let backups = scan_backup_directory(now);
    let runtime = runtime_snapshot();
    AdminServerMonitorResponse {
        server,
        database,
        backups,
        runtime,
    }
}

async fn authenticated_principal_for_live(
    state: &AppState,
    headers: &HeaderMap,
    query_token: &str,
) -> Result<crate::core::auth::models::Principal, AdminError> {
    let token = query_token.trim().to_string();
    let token = if token.is_empty() {
        bearer_token(headers).ok_or_else(unauthorized)?
    } else {
        token
    };
    state.sessions.get(&token).await.map_err(|_| unauthorized())
}

async fn system_monitor_live_socket(state: AppState, mut socket: WebSocket) {
    ensure_system_monitor_hub_started(&state);
    let mut snapshots = state.system_monitor_hub.subscribe();
    let mut heartbeat = tokio::time::interval(LIVE_HEARTBEAT_INTERVAL);

    let initial_report = { snapshots.borrow().clone() };
    if let Some(report) = initial_report {
        if !send_system_monitor_snapshot(&mut socket, report).await {
            return;
        }
    }

    loop {
        tokio::select! {
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(pong) = system_monitor_pong_text(&text) {
                            if !send_system_monitor_message(&mut socket, Message::Text(pong.into())).await {
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::warn!(%error, "system monitor live message receive failed");
                        break;
                    }
                }
            }
            changed = snapshots.changed() => {
                if changed.is_err() {
                    break;
                }
                let report = { snapshots.borrow().clone() };
                if let Some(report) = report {
                    if !send_system_monitor_snapshot(&mut socket, report).await {
                        break;
                    }
                }
            }
            _ = heartbeat.tick() => {
                if !send_system_monitor_message(&mut socket, Message::Ping(Vec::new().into())).await {
                    break;
                }
            }
        }
    }
}

fn system_monitor_pong_text(text: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(text).ok()?;
    if value.get("type").and_then(|item| item.as_str()) != Some("ping") {
        return None;
    }
    let payload = serde_json::json!({
        "type": "pong",
        "id": value.get("id").cloned().unwrap_or(serde_json::Value::Null),
        "sent_at_ms": value.get("sent_at_ms").cloned().unwrap_or(serde_json::Value::Null),
        "server_at_ms": system_time_to_unix_ms(SystemTime::now()),
    });
    serde_json::to_string(&payload).ok()
}

fn ensure_system_monitor_hub_started(state: &AppState) {
    if !state.system_monitor_hub.mark_started() {
        return;
    }

    let state = state.clone();
    let hub = state.system_monitor_hub.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(LIVE_SNAPSHOT_INTERVAL);
        loop {
            interval.tick().await;
            hub.publish(system_monitor_report(&state).await);
        }
    });
}

async fn send_system_monitor_snapshot(
    socket: &mut WebSocket,
    report: AdminServerMonitorResponse,
) -> bool {
    let payload = serde_json::json!({
        "ok": true,
        "server": report.server,
        "database": report.database,
        "backups": report.backups,
        "runtime": report.runtime,
    });
    match serde_json::to_string(&payload) {
        Ok(json) => send_system_monitor_message(socket, Message::Text(json.into())).await,
        Err(error) => {
            tracing::warn!(%error, "system monitor live snapshot serialization failed");
            true
        }
    }
}

async fn send_system_monitor_message(socket: &mut WebSocket, message: Message) -> bool {
    match timeout(LIVE_SEND_TIMEOUT, socket.send(message)).await {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            tracing::warn!(%error, "system monitor live message send failed");
            false
        }
        Err(_) => {
            tracing::warn!("system monitor live message send timed out");
            false
        }
    }
}

enum DatabasePingOutcome {
    Online,
    Error(String),
    Timeout,
}

fn database_monitor_from_ping_outcome(
    started: Instant,
    outcome: DatabasePingOutcome,
) -> AdminServerMonitorDatabase {
    match outcome {
        DatabasePingOutcome::Online => AdminServerMonitorDatabase {
            configured: true,
            reachable: true,
            status: "online".to_string(),
            ping_ms: elapsed_ping_ms(started),
            error: String::new(),
        },
        DatabasePingOutcome::Error(error) => AdminServerMonitorDatabase {
            configured: true,
            reachable: false,
            status: "offline".to_string(),
            ping_ms: elapsed_ping_ms(started),
            error,
        },
        DatabasePingOutcome::Timeout => AdminServerMonitorDatabase {
            configured: true,
            reachable: false,
            status: "offline".to_string(),
            ping_ms: elapsed_ping_ms(started),
            error: "database ping timed out".to_string(),
        },
    }
}

fn scan_backup_directory(now: OffsetDateTime) -> AdminServerMonitorBackups {
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
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        snapshot.file_count += 1;
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        files.push((modified, entry.path(), metadata.len()));
    }
    files.sort_by(|left, right| right.0.cmp(&left.0));
    snapshot.files = files
        .into_iter()
        .map(|(modified, path, size_bytes)| backup_file_snapshot(now, modified, path, size_bytes))
        .collect();
    snapshot.latest = snapshot.files.first().cloned();
    snapshot
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

fn runtime_snapshot() -> AdminServerMonitorRuntime {
    let (memory_used_mb, memory_total_mb, memory_percent) = memory_snapshot();
    let disk = disk_snapshot();
    let load_average = load_average_snapshot();
    let cpu_percent = cpu_pressure_percent(load_average);
    AdminServerMonitorRuntime {
        cpu_percent,
        memory_percent,
        memory_used_mb,
        memory_total_mb,
        disk_path: disk.path,
        disk_percent: disk.percent,
        disk_used_mb: disk.used_mb,
        disk_total_mb: disk.total_mb,
        disk_available_mb: disk.available_mb,
        load_average,
        sample_seconds: LIVE_SNAPSHOT_INTERVAL.as_secs().min(i64::MAX as u64) as i64,
    }
}

struct DiskSnapshot {
    path: String,
    percent: i64,
    used_mb: i64,
    total_mb: i64,
    available_mb: i64,
}

fn memory_snapshot() -> (i64, i64, i64) {
    let Ok(meminfo) = fs::read_to_string("/proc/meminfo") else {
        return (0, 0, 0);
    };
    let total_kb = meminfo_value_kb(&meminfo, "MemTotal").unwrap_or(0);
    let available_kb = meminfo_value_kb(&meminfo, "MemAvailable").unwrap_or(0);
    if total_kb <= 0 {
        return (0, 0, 0);
    }
    let used_kb = total_kb.saturating_sub(available_kb);
    let used_mb = used_kb / 1024;
    let total_mb = total_kb / 1024;
    let percent = ((used_kb as f64 / total_kb as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as i64;
    (used_mb, total_mb, percent)
}

fn meminfo_value_kb(meminfo: &str, key: &str) -> Option<i64> {
    meminfo.lines().find_map(|line| {
        let (name, rest) = line.split_once(':')?;
        if name != key {
            return None;
        }
        rest.split_whitespace().next()?.parse::<i64>().ok()
    })
}

fn load_average_snapshot() -> f64 {
    fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|value| value.split_whitespace().next()?.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(0.0)
}

fn cpu_pressure_percent(load_average: f64) -> i64 {
    if load_average <= 0.0 {
        return 0;
    }
    let cores = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1)
        .max(1) as f64;
    ((load_average / cores) * 100.0).round().clamp(0.0, 100.0) as i64
}

fn disk_snapshot() -> DiskSnapshot {
    let path = disk_monitor_path();
    let display_path = path.display().to_string();
    let Ok(output) = Command::new("df").arg("-Pk").arg(&path).output() else {
        return empty_disk_snapshot(display_path);
    };
    if !output.status.success() {
        return empty_disk_snapshot(display_path);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(line) = stdout.lines().nth(1) else {
        return empty_disk_snapshot(display_path);
    };
    parse_df_line(&display_path, line).unwrap_or_else(|| empty_disk_snapshot(display_path))
}

fn disk_monitor_path() -> PathBuf {
    std::env::var("MINI_ERP_DISK_MONITOR_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn parse_df_line(path: &str, line: &str) -> Option<DiskSnapshot> {
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 5 {
        return None;
    }
    let total_kb = fields.get(1)?.parse::<i64>().ok()?.max(0);
    let used_kb = fields.get(2)?.parse::<i64>().ok()?.max(0);
    let available_kb = fields.get(3)?.parse::<i64>().ok()?.max(0);
    let percent = fields
        .get(4)?
        .trim_end_matches('%')
        .parse::<i64>()
        .ok()
        .unwrap_or_else(|| {
            if total_kb <= 0 {
                0
            } else {
                ((used_kb as f64 / total_kb as f64) * 100.0).round() as i64
            }
        })
        .clamp(0, 100);
    Some(DiskSnapshot {
        path: path.to_string(),
        percent,
        used_mb: used_kb / 1024,
        total_mb: total_kb / 1024,
        available_mb: available_kb / 1024,
    })
}

fn empty_disk_snapshot(path: String) -> DiskSnapshot {
    DiskSnapshot {
        path,
        percent: 0,
        used_mb: 0,
        total_mb: 0,
        available_mb: 0,
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

fn first_existing_backup_directory<const N: usize>(candidates: [PathBuf; N]) -> PathBuf {
    let fallback = candidates
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("backups/mini_rs_erp_db"));
    candidates
        .into_iter()
        .find(|candidate| candidate.is_dir())
        .unwrap_or(fallback)
}

fn elapsed_ping_ms(started: Instant) -> i64 {
    let elapsed = started.elapsed().as_millis().min(i64::MAX as u128) as i64;
    elapsed.max(1)
}

fn system_time_to_unix(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

fn system_time_to_unix_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let database =
            database_monitor_from_ping_outcome(Instant::now(), DatabasePingOutcome::Timeout);

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
}
