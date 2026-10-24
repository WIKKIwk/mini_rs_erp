use super::*;
use crate::core::admin::models::{
    AdminServerMonitorDatabase, AdminServerMonitorResponse, AdminServerMonitorServer,
};
use crate::http::ws::{Message, WebSocket, WebSocketUpgrade};
#[cfg(test)]
use std::fs;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;
use tokio::time::{Duration, timeout};

mod backup;
mod backup_api;
#[cfg(test)]
mod tests;

pub use backup_api::{system_backup_create, system_backup_download, system_backup_import};

const DATABASE_PING_TIMEOUT: Duration = Duration::from_secs(2);
const LIVE_SNAPSHOT_INTERVAL: Duration = Duration::from_secs(2);
const LIVE_SEND_TIMEOUT: Duration = Duration::from_secs(5);

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
    let cached = state.system_monitor_hub.subscribe().borrow().clone();
    if let Some(report) = cached {
        return report;
    }
    start_backup_catalog(state);
    let mut backups = state.system_monitor_hub.subscribe_backups();
    // A read must not advance failure counters. Only the periodic sampler does that.
    let mut report = collect_monitor_report(state).await;
    if backups.borrow().is_none() {
        let _ = timeout(Duration::from_secs(2), backups.changed()).await;
    }
    if let Some(snapshot) = backups.borrow().clone() {
        report.backups = snapshot;
    }
    report
}

async fn collect_monitor_report(state: &AppState) -> AdminServerMonitorResponse {
    let uptime_seconds = state.started_at.elapsed().as_secs().min(i64::MAX as u64) as i64;
    let server = AdminServerMonitorServer {
        bind_addr: state.config.bind_addr.to_string(),
        started_at_unix: state.started_at_unix,
        uptime_seconds,
        status: "running".to_string(),
    };
    let (database, mut runtime) = tokio::join!(
        database_monitor(state),
        crate::core::system_metrics::collect(),
    );
    runtime.sample_seconds = LIVE_SNAPSHOT_INTERVAL.as_secs() as i64;
    let backups = state
        .system_monitor_hub
        .subscribe_backups()
        .borrow()
        .clone()
        .unwrap_or_else(|| crate::core::admin::models::AdminServerMonitorBackups {
            error: "backup monitoring pending".to_string(),
            ..Default::default()
        });
    AdminServerMonitorResponse {
        server,
        database,
        backups,
        runtime,
        doctor: state.backup_doctor.system_doctor().snapshot(),
    }
}

async fn database_monitor(state: &AppState) -> AdminServerMonitorDatabase {
    match &state.mini_engine {
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
    start_system_doctor(&state);
    let mut snapshots = state.system_monitor_hub.subscribe();

    let initial_report = { snapshots.borrow().clone() };
    if let Some(report) = initial_report
        && !send_system_monitor_snapshot(&mut socket, report).await
    {
        return;
    }

    loop {
        tokio::select! {
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(pong) = system_monitor_pong_text(&text)
                            && !send_system_monitor_message(&mut socket, Message::Text(pong.into())).await {
                            break;
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
                if let Some(report) = report
                    && !send_system_monitor_snapshot(&mut socket, report).await {
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

pub fn start_system_doctor(state: &AppState) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::warn!("system doctor requires a Tokio runtime");
        return;
    };
    if !state.system_monitor_hub.mark_started() {
        return;
    }

    start_backup_catalog(state);
    let state = state.clone();
    crate::core::system_doctor::domain::runtime::start(state.backup_doctor.system_doctor());
    crate::core::system_doctor::wifi::runtime::start(state.backup_doctor.system_doctor());
    let hub = state.system_monitor_hub.clone();
    handle.spawn(async move {
        let mut interval = tokio::time::interval(LIVE_SNAPSHOT_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let sampled_at = OffsetDateTime::now_utc().unix_timestamp();
            let mut report = collect_monitor_report(&state).await;
            report.doctor = state.backup_doctor.system_doctor().observe(
                &report.database,
                &report.runtime,
                sampled_at,
            );
            hub.publish(report);
        }
    });
}

fn start_backup_catalog(state: &AppState) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    if !state.system_monitor_hub.mark_backup_started() {
        return;
    }
    let doctor = state.backup_doctor.clone();
    let hub = state.system_monitor_hub.clone();
    handle.spawn(async move {
        loop {
            let doctor = doctor.clone();
            // Only one scan can run. A stuck catalogue cannot stall health
            // observations or accumulate abandoned blocking workers.
            let snapshot =
                tokio::task::spawn_blocking(move || doctor.report(OffsetDateTime::now_utc()))
                    .await
                    .unwrap_or_else(|_| crate::core::admin::models::AdminServerMonitorBackups {
                        error: "backup monitoring unavailable".to_string(),
                        ..Default::default()
                    });
            hub.publish_backups(snapshot);
            tokio::time::sleep(LIVE_SNAPSHOT_INTERVAL).await;
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
        "doctor": report.doctor,
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

fn elapsed_ping_ms(started: Instant) -> i64 {
    let elapsed = started.elapsed().as_millis().min(i64::MAX as u128) as i64;
    elapsed.max(1)
}

fn system_time_to_unix_ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
