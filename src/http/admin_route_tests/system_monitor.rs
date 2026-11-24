use super::*;

#[tokio::test]
async fn system_doctor_samples_without_an_open_admin_screen() {
    let state = test_state();
    crate::http::handlers::admin::start_system_doctor(&state);
    // Subscribe only to inspect the first background sample, not to start it.
    let mut samples = state.system_monitor_hub.subscribe();
    if samples.borrow().is_none() {
        tokio::time::timeout(Duration::from_secs(6), samples.changed())
            .await
            .expect("doctor sample timeout")
            .expect("doctor sample");
    }
    assert!(samples.borrow().as_ref().unwrap().doctor.checked_at_unix > 0);
}

#[tokio::test]
async fn system_doctor_sampling_continues_while_backup_catalog_is_pending() {
    let state = test_state();
    // Reserve the scan worker without publishing a result, as with slow storage.
    assert!(state.system_monitor_hub.mark_backup_started());
    crate::http::handlers::admin::start_system_doctor(&state);
    let mut samples = state.system_monitor_hub.subscribe();
    if samples.borrow().is_none() {
        tokio::time::timeout(Duration::from_secs(6), samples.changed())
            .await
            .expect("independent doctor timeout")
            .expect("sample");
    }
    let report = samples.borrow().clone().unwrap();
    assert!(report.doctor.checked_at_unix > 0);
    assert_eq!(report.backups.error, "backup monitoring pending");
}

#[tokio::test]
async fn system_doctor_resource_pressure_rejects_backup_and_import_over_http() {
    use crate::core::admin::models::{AdminServerMonitorDatabase, AdminServerMonitorRuntime};
    let state = test_state();
    let doctor = state.backup_doctor.system_doctor();
    let database = AdminServerMonitorDatabase {
        configured: true,
        reachable: true,
        ..Default::default()
    };
    let runtime = AdminServerMonitorRuntime {
        memory_total_mb: 8192,
        memory_percent: 99,
        disk_total_mb: 100_000,
        disk_available_mb: 50_000,
        ..Default::default()
    };
    for _ in 0..60 {
        doctor.observe(
            &database,
            &runtime,
            time::OffsetDateTime::now_utc().unix_timestamp(),
        );
    }
    assert!(doctor.protected());
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);
    for path in [
        "/v1/mobile/admin/system/backups",
        "/v1/mobile/admin/system/backups/import",
    ] {
        let response = router
            .clone()
            .oneshot(request("POST", path, &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            json_body(response).await["error"],
            "backup_resource_pressure"
        );
    }
}

#[tokio::test]
async fn system_doctor_readiness_is_separate_from_liveness() {
    use crate::core::admin::models::AdminServerMonitorResponse;
    let state = test_state();
    let router = build_router(state.clone());
    let response = router
        .clone()
        .oneshot(request("GET", "/readyz", ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let response = router
        .clone()
        .oneshot(request("GET", "/healthz", ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut report = AdminServerMonitorResponse::default();
    report.database.configured = true;
    report.database.reachable = true;
    report.doctor.checked_at_unix = time::OffsetDateTime::now_utc().unix_timestamp();
    state.system_monitor_hub.publish(report.clone());
    let response = router
        .clone()
        .oneshot(request("GET", "/readyz", ""))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    report.doctor.protected = true;
    state.system_monitor_hub.publish(report);
    let response = router.oneshot(request("GET", "/readyz", "")).await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json_body(response).await, serde_json::json!({"ok": false}));
}

#[tokio::test]
async fn system_doctor_readiness_waits_for_confirmed_database_recovery() {
    use crate::core::admin::models::AdminServerMonitorResponse;
    let state = test_state();
    let router = build_router(state.clone());
    let doctor = crate::core::system_doctor::SystemDoctor::default();
    let mut report = AdminServerMonitorResponse::default();
    report.database.configured = true;
    report.runtime.memory_total_mb = 8192;
    report.runtime.memory_percent = 50;
    report.runtime.disk_total_mb = 100_000;
    report.runtime.disk_available_mb = 50_000;
    for _ in 0..3 {
        doctor.observe(
            &report.database,
            &report.runtime,
            time::OffsetDateTime::now_utc().unix_timestamp(),
        );
    }
    report.database.reachable = true;
    for expected in [StatusCode::SERVICE_UNAVAILABLE, StatusCode::OK] {
        report.doctor = doctor.observe(
            &report.database,
            &report.runtime,
            time::OffsetDateTime::now_utc().unix_timestamp(),
        );
        state.system_monitor_hub.publish(report.clone());
        let response = router
            .clone()
            .oneshot(request("GET", "/readyz", ""))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
}

#[tokio::test]
async fn system_doctor_monitor_reads_do_not_advance_failure_counters() {
    let state = test_state();
    let doctor = state.backup_doctor.system_doctor();
    let before = doctor.snapshot();
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);
    for _ in 0..2 {
        let response = router
            .clone()
            .oneshot(request("GET", "/v1/mobile/admin/system/monitor", &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        assert_eq!(body["doctor"]["status"], "starting");
    }
    assert_eq!(doctor.snapshot(), before);
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            unsafe {
                std::env::set_var(self.key, previous);
            }
        } else {
            unsafe {
                std::env::remove_var(self.key);
            }
        }
    }
}

#[tokio::test]
async fn admin_system_monitor_reports_server_and_backup_state() {
    let backup_dir = tempfile::tempdir().expect("backup dir");
    let nested_dir = backup_dir.path().join("20260624_180448");
    std::fs::create_dir_all(&nested_dir).expect("nested backup dir");
    let backup_file = nested_dir.join("mini_rs_erp_20260624_180448.dump");
    std::fs::write(&backup_file, b"backup-bytes").expect("write backup");
    std::fs::write(nested_dir.join(".env.deploy"), b"not-db-backup").expect("write env backup");
    std::fs::write(nested_dir.join("mini_rs_erp"), b"not-db-backup").expect("write binary backup");
    let _guard = EnvVarGuard::set("MINI_ERP_BACKUP_DIR", backup_dir.path());
    let _disk_guard = EnvVarGuard::set("MINI_ERP_DISK_MONITOR_PATH", backup_dir.path());
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);

    let response = router
        .oneshot(request("GET", "/v1/mobile/admin/system/monitor", &token))
        .await
        .expect("monitor response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;

    assert_eq!(body["server"]["bind_addr"], "127.0.0.1:8081");
    assert_eq!(body["server"]["status"], "running");
    assert!(body["server"]["started_at_unix"].as_i64().unwrap_or(0) > 0);
    assert!(body["server"]["uptime_seconds"].as_i64().unwrap_or(-1) >= 0);

    assert_eq!(
        body["backups"]["directory"],
        backup_dir.path().display().to_string()
    );
    assert_eq!(body["backups"]["exists"], true);
    assert_eq!(body["backups"]["file_count"], 1);
    assert_eq!(
        body["backups"]["latest"]["name"].as_str().unwrap_or(""),
        backup_file
            .file_name()
            .expect("backup file name")
            .to_str()
            .unwrap_or("")
    );
    assert_eq!(
        body["backups"]["files"][0]["name"].as_str().unwrap_or(""),
        backup_file
            .file_name()
            .expect("backup file name")
            .to_str()
            .unwrap_or("")
    );
    assert!(
        body["backups"]["latest"]["age_seconds"]
            .as_i64()
            .unwrap_or(-1)
            >= 0
    );

    assert!(!body["database"]["status"].as_str().unwrap_or("").is_empty());
    assert!(body["runtime"]["cpu_percent"].as_i64().unwrap_or(-1) >= 0);
    assert!(body["runtime"]["memory_percent"].as_i64().unwrap_or(-1) >= 0);
    assert!(body["runtime"]["memory_used_mb"].as_i64().unwrap_or(-1) >= 0);
    assert!(body["runtime"]["memory_total_mb"].as_i64().unwrap_or(-1) >= 0);
    assert!(body["runtime"]["load_average"].as_f64().unwrap_or(-1.0) >= 0.0);
    assert!(body["runtime"]["sample_seconds"].as_i64().unwrap_or(-1) >= 0);
    assert_eq!(
        body["runtime"]["disk_path"].as_str().unwrap_or(""),
        backup_dir.path().display().to_string()
    );
    assert!(body["runtime"]["disk_percent"].as_i64().unwrap_or(-1) >= 0);
    assert!(body["runtime"]["disk_used_mb"].as_i64().unwrap_or(-1) >= 0);
    assert!(body["runtime"]["disk_total_mb"].as_i64().unwrap_or(-1) >= 0);
    assert!(body["runtime"]["disk_available_mb"].as_i64().unwrap_or(-1) >= 0);
}

#[tokio::test]
async fn admin_starts_guarded_backup_and_downloads_verified_dump() {
    let backup_dir = tempfile::tempdir().expect("backup dir");
    let script_dir = tempfile::tempdir().expect("script dir");
    let script = script_dir.path().join("fake_backup.sh");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env bash
set -euo pipefail
dir="$MINI_ERP_BACKUP_DIR/$MINI_ERP_BACKUP_TIMESTAMP"
mkdir -p "$dir"
sleep 1
printf 'verified-backup-bytes' > "$dir/mini_rs_erp.dump"
printf '%s\n' "$dir"
"#,
    )
    .expect("fake backup script");

    let mut state = test_state();
    state.backup_doctor = crate::core::backup_doctor::BackupDoctor::for_test(
        backup_dir.path(),
        &script,
        "postgres://test",
    );
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state.clone());

    let started = router
        .clone()
        .oneshot(request("POST", "/v1/mobile/admin/system/backups", &token))
        .await
        .expect("start backup");
    assert_eq!(started.status(), StatusCode::ACCEPTED);
    let started_body = json_body(started).await;
    let backup_id = started_body["id"].as_str().unwrap_or_default().to_string();
    assert!(!backup_id.is_empty());
    assert_eq!(started_body["status"], "queued");

    let pending_download = router
        .clone()
        .oneshot(request(
            "GET",
            &format!("/v1/mobile/admin/system/backups/{backup_id}/download"),
            &token,
        ))
        .await
        .expect("pending download");
    assert_eq!(pending_download.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(pending_download).await["error"],
        "backup_not_ready"
    );

    let duplicate = router
        .clone()
        .oneshot(request("POST", "/v1/mobile/admin/system/backups", &token))
        .await
        .expect("duplicate backup");
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(duplicate).await["error"],
        "backup_already_running"
    );

    let mut ready = false;
    for _ in 0..40 {
        let report = state.backup_doctor.report(time::OffsetDateTime::now_utc());
        ready = report
            .snapshots
            .iter()
            .any(|snapshot| snapshot.id == backup_id && snapshot.status == "ready");
        if ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "backup did not become ready");

    let repeated = router
        .clone()
        .oneshot(request("POST", "/v1/mobile/admin/system/backups", &token))
        .await
        .expect("repeat backup");
    assert_eq!(repeated.status(), StatusCode::ACCEPTED);
    let repeated_id = json_body(repeated).await["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(!repeated_id.is_empty());
    for _ in 0..40 {
        let repeated_ready = state
            .backup_doctor
            .report(time::OffsetDateTime::now_utc())
            .snapshots
            .iter()
            .any(|snapshot| snapshot.id == repeated_id && snapshot.status == "ready");
        if repeated_ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        state
            .backup_doctor
            .report(time::OffsetDateTime::now_utc())
            .snapshots
            .iter()
            .any(|snapshot| snapshot.id == repeated_id && snapshot.status == "ready")
    );

    let download = router
        .oneshot(request(
            "GET",
            &format!("/v1/mobile/admin/system/backups/{backup_id}/download"),
            &token,
        ))
        .await
        .expect("download backup");
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(
        download
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok()),
        Some("attachment; filename=\"mini_rs_erp.dump\"")
    );
    let bytes = to_bytes(download.into_body(), usize::MAX)
        .await
        .expect("download bytes");
    assert_eq!(&bytes[..], b"verified-backup-bytes");
}

#[tokio::test]
async fn admin_imports_mobile_dump_and_exposes_verified_snapshot() {
    let backup_dir = tempfile::tempdir().expect("backup dir");
    let backup_script_dir = tempfile::tempdir().expect("backup script dir");
    let backup_script = backup_script_dir.path().join("fake_backup.sh");
    std::fs::write(
        &backup_script,
        r#"#!/usr/bin/env bash
set -euo pipefail
dir="$MINI_ERP_BACKUP_DIR/$MINI_ERP_BACKUP_TIMESTAMP"
mkdir -p "$dir"
printf 'pre-restore-backup-bytes' > "$dir/mini_rs_erp.dump"
"#,
    )
    .expect("write fake backup script");
    let restore_script_dir = tempfile::tempdir().expect("restore script dir");
    let restore_script = restore_script_dir.path().join("fake_restore.sh");
    std::fs::write(
        &restore_script,
        r#"#!/usr/bin/env bash
set -euo pipefail
test -s "$MINI_ERP_RESTORE_DUMP"
backup_root="$(dirname "$(dirname "$MINI_ERP_RESTORE_DUMP")")"
test -n "$(find "$backup_root" -maxdepth 1 -type d -name 'backup-pre-restore-*' -print -quit)"
printf 'restore-ran' > "${MINI_ERP_RESTORE_DUMP}.restored"
"#,
    )
    .expect("write restore script");

    let mut state = test_state();
    state.backup_doctor = crate::core::backup_doctor::BackupDoctor::for_test_with_restore(
        backup_dir.path(),
        &backup_script,
        &restore_script,
        "postgres://test",
    );
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state.clone());
    let import = Request::builder()
        .method("POST")
        .uri("/v1/mobile/admin/system/backups/import")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header("x-backup-filename", "mobile-export.dump")
        .body(Body::from("mobile-dump-bytes"))
        .expect("import request");
    let response = router
        .clone()
        .oneshot(import)
        .await
        .expect("import response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let started = json_body(response).await;
    let import_id = started["id"].as_str().unwrap_or_default().to_string();
    assert_eq!(started["source"], "imported");
    assert_eq!(started["status"], "queued");

    let mut ready = false;
    for _ in 0..40 {
        ready = state
            .backup_doctor
            .report(time::OffsetDateTime::now_utc())
            .snapshots
            .iter()
            .any(|snapshot| {
                snapshot.id == import_id && snapshot.status == "ready" && snapshot.verified
            });
        if ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "import did not become ready");
    let snapshots = state
        .backup_doctor
        .report(time::OffsetDateTime::now_utc())
        .snapshots;
    assert!(snapshots.iter().any(|snapshot| {
        snapshot.source == "pre_restore" && snapshot.status == "ready" && snapshot.verified
    }));

    let download = router
        .oneshot(request(
            "GET",
            &format!("/v1/mobile/admin/system/backups/{import_id}/download"),
            &token,
        ))
        .await
        .expect("download imported backup");
    assert_eq!(download.status(), StatusCode::OK);
    let bytes = to_bytes(download.into_body(), usize::MAX)
        .await
        .expect("download imported bytes");
    assert_eq!(&bytes[..], b"mobile-dump-bytes");
}

#[tokio::test]
async fn admin_import_rolls_back_when_post_restore_migration_fails() {
    let backup_dir = tempfile::tempdir().expect("backup dir");
    let backup_script_dir = tempfile::tempdir().expect("backup script dir");
    let backup_script = backup_script_dir.path().join("fake_backup.sh");
    std::fs::write(
        &backup_script,
        r#"#!/usr/bin/env bash
set -euo pipefail
dir="$MINI_ERP_BACKUP_DIR/$MINI_ERP_BACKUP_TIMESTAMP"
mkdir -p "$dir"
printf 'pre-restore-backup-bytes' > "$dir/mini_rs_erp.dump"
"#,
    )
    .expect("write fake backup script");
    let restore_script_dir = tempfile::tempdir().expect("restore script dir");
    let restore_script = restore_script_dir.path().join("fake_restore.sh");
    std::fs::write(
        &restore_script,
        r#"#!/usr/bin/env bash
set -euo pipefail
test -s "$MINI_ERP_RESTORE_DUMP"
backup_root="$(dirname "$(dirname "$MINI_ERP_RESTORE_DUMP")")"
printf '%s\n' "$MINI_ERP_RESTORE_DUMP" >> "$backup_root/restore-calls.log"
"#,
    )
    .expect("write restore script");

    let mut state = test_state();
    state.backup_doctor =
        crate::core::backup_doctor::BackupDoctor::for_test_with_restore_and_migration(
            backup_dir.path(),
            &backup_script,
            &restore_script,
            "not-a-postgres-url",
        );
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state.clone());
    let import = Request::builder()
        .method("POST")
        .uri("/v1/mobile/admin/system/backups/import")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header("x-backup-filename", "mobile-export.dump")
        .body(Body::from("mobile-dump-bytes"))
        .expect("import request");
    let response = router.oneshot(import).await.expect("import response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let import_id = json_body(response).await["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    let mut failed = false;
    for _ in 0..40 {
        failed = state
            .backup_doctor
            .report(time::OffsetDateTime::now_utc())
            .snapshots
            .iter()
            .any(|snapshot| snapshot.id == import_id && snapshot.status == "failed");
        if failed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        failed,
        "import did not fail after migration connection refusal"
    );

    let calls = std::fs::read_to_string(backup_dir.path().join("restore-calls.log"))
        .expect("restore calls log");
    assert_eq!(calls.lines().count(), 2);
    assert!(
        calls
            .lines()
            .nth(1)
            .unwrap_or_default()
            .contains("backup-pre-restore-")
    );
    let snapshots = state
        .backup_doctor
        .report(time::OffsetDateTime::now_utc())
        .snapshots;
    let failed_import = snapshots
        .iter()
        .find(|snapshot| snapshot.id == import_id)
        .expect("failed import snapshot");
    assert!(failed_import.error.contains("migration"));
    assert!(snapshots.iter().any(|snapshot| {
        snapshot.source == "pre_restore" && snapshot.status == "ready" && snapshot.verified
    }));
}

fn live_upgrade(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request.header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
}

async fn open_live(request: reqwest::RequestBuilder) -> reqwest::Upgraded {
    let response = live_upgrade(request).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    response.upgrade().await.unwrap()
}

async fn assert_live_ended(socket: &mut reqwest::Upgraded) {
    use tokio::io::AsyncReadExt;
    match tokio::time::timeout(Duration::from_secs(3), socket.read_u8())
        .await.expect("revoked socket should close before another payload") {
        Ok(first) => assert_eq!(first & 0x0f, 8, "expected a WebSocket close frame"),
        Err(error) => assert!(matches!(error.kind(),
            std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset)),
    }
}

#[tokio::test]
async fn monitor_and_chat_streams_stop_after_the_issuing_session_is_revoked() {
    use crate::core::session::{JsonSessionStore, SessionStore};
    let mut state = test_state();
    let sessions = Arc::new(JsonSessionStore::memory());
    state.sessions = SessionManager::with_store(sessions.clone(), Some(3600));
    state.chat = crate::core::chat::ChatService::unavailable();
    assert!(state.system_monitor_hub.mark_started());
    state.system_monitor_hub.publish(Default::default());
    let token = session(&state, PrincipalRole::Admin).await;
    let other_token = session(&state, PrincipalRole::Admin).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1/mobile", listener.local_addr().unwrap());
    let service = build_router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    let client = reqwest::Client::new();
    let monitor = format!("{base}/admin/system/monitor/live");
    let mut first_monitor = open_live(client.get(&monitor).bearer_auth(&token)).await;
    let mut other_monitor = open_live(client.get(&monitor).bearer_auth(&other_token)).await;
    assert_eq!(super::inventory_live::message(&mut first_monitor).await["ok"], true);
    assert_eq!(super::inventory_live::message(&mut other_monitor).await["ok"], true);

    let issue_chat = || async {
        let ticket = client.post(format!("{base}/chat/socket-ticket")).bearer_auth(&token)
            .send().await.unwrap().error_for_status().unwrap().json::<serde_json::Value>().await.unwrap();
        ticket["ticket"].as_str().unwrap().to_owned()
    };
    let live_chat = |ticket: &str| client.get(format!("{base}/chat/live")).query(&[("ticket", ticket)]);
    let used = issue_chat().await;
    let outstanding = issue_chat().await;
    let mut chat = open_live(live_chat(&used)).await;
    assert_eq!(super::inventory_live::message(&mut chat).await["event"], "chat.ready");
    assert_eq!(live_upgrade(live_chat(&used)).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);

    state.sessions.delete(&token).await.unwrap();
    state.system_monitor_hub.publish(Default::default());
    let event = serde_json::from_value(serde_json::json!({
        "event_id": "event", "event": "chat.message.created", "conversation_id": "room", "sequence": 1,
        "message": {"message_id":"message","conversation_id":"room","sender_principal_id":"sender",
            "sender_role":"admin","sender_ref":"admin","sender_display_name":"Admin","client_message_id":"client",
            "sequence":1,"type":"text","body":"private","created_at_unix":1}
    })).unwrap();
    state.chat.hub().publish(event, &["admin:admin".to_string()]).await;
    assert_live_ended(&mut first_monitor).await;
    assert_live_ended(&mut chat).await;
    assert_eq!(super::inventory_live::message(&mut other_monitor).await["ok"], true);
    assert_eq!(live_upgrade(live_chat(&outstanding)).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);

    // Expire the still-open connection's backing record without changing its
    // process-local cache or removing the principal's account.
    let mut record = sessions.get(&other_token).await.unwrap().unwrap();
    record.expires_at = Some(time::OffsetDateTime::UNIX_EPOCH);
    sessions.put(&other_token, &record).await.unwrap();
    state.system_monitor_hub.publish(Default::default());
    assert_live_ended(&mut other_monitor).await;
    server.abort();
}

#[tokio::test]
async fn monitor_with_valid_session_stops_when_admin_access_is_removed() {
    let mut state = test_state();
    assert!(state.system_monitor_hub.mark_started());
    state.system_monitor_hub.publish(Default::default());
    let roles = Arc::new(MemoryRoleDefinitionStore::new());
    let mut role = RoleDefinition {
        id: "monitor-reader".into(), label: "Monitor reader".into(),
        base_role: None, system: false, capability_codes: vec!["admin.access".into()],
    };
    roles.put_role_definition(role.clone()).await.unwrap();
    roles.put_role_assignment(RoleAssignment {
        principal_role: PrincipalRole::Supplier, principal_ref: "SUP-001".into(),
        role_id: role.id.clone(), assigned_apparatus: vec![], assigned_item_groups: vec![],
    }).await.unwrap();
    state.admin = state.admin.clone().with_role_store(roles.clone());
    let token = session_for(&state, PrincipalRole::Supplier, "SUP-001").await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri = format!("http://{}/v1/mobile/admin/system/monitor/live", listener.local_addr().unwrap());
    let service = build_router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    let mut socket = open_live(reqwest::Client::new().get(uri).bearer_auth(&token)).await;
    assert_eq!(super::inventory_live::message(&mut socket).await["ok"], true);
    role.capability_codes.clear();
    roles.put_role_definition(role).await.unwrap();
    state.system_monitor_hub.publish(Default::default());
    assert_live_ended(&mut socket).await;
    assert!(state.sessions.get(&token).await.is_ok());
    server.abort();
}
