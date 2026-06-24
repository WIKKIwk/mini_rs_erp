use super::*;
use crate::core::admin::models::{
    AdminServerMonitorBackupFile, AdminServerMonitorBackups, AdminServerMonitorDatabase,
    AdminServerMonitorResponse, AdminServerMonitorServer,
};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use std::fs;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;
use tokio::time::Duration;

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
            match engine.ping().await {
                Ok(()) => AdminServerMonitorDatabase {
                    configured: true,
                    reachable: true,
                    status: "online".to_string(),
                    ping_ms: elapsed_ping_ms(started),
                    error: String::new(),
                },
                Err(error) => AdminServerMonitorDatabase {
                    configured: true,
                    reachable: false,
                    status: "offline".to_string(),
                    ping_ms: elapsed_ping_ms(started),
                    error: error.to_string(),
                },
            }
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
    AdminServerMonitorResponse {
        server,
        database,
        backups,
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
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    loop {
        interval.tick().await;
        let report = system_monitor_report(&state).await;
        let payload = serde_json::json!({
            "ok": true,
            "server": report.server,
            "database": report.database,
            "backups": report.backups,
        });
        match serde_json::to_string(&payload) {
            Ok(json) => {
                if let Err(error) = socket.send(Message::Text(json.into())).await {
                    tracing::warn!(%error, "system monitor live snapshot send failed");
                    break;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "system monitor live snapshot serialization failed");
            }
        }
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
    let mut latest: Option<(SystemTime, PathBuf, u64)> = None;
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
        match &latest {
            Some((current_modified, _, _)) if modified <= *current_modified => {}
            _ => {
                latest = Some((modified, entry.path(), metadata.len()));
            }
        }
    }
    snapshot.latest = latest.map(|(modified, path, size_bytes)| {
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
    });
    snapshot
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
