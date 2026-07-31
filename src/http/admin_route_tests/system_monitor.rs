use super::*;

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
    let restore_script_dir = tempfile::tempdir().expect("restore script dir");
    let restore_script = restore_script_dir.path().join("fake_restore.sh");
    std::fs::write(
        &restore_script,
        r#"#!/usr/bin/env bash
set -euo pipefail
test -s "$MINI_ERP_RESTORE_DUMP"
printf 'restore-ran' > "${MINI_ERP_RESTORE_DUMP}.restored"
"#,
    )
    .expect("write restore script");

    let mut state = test_state();
    state.backup_doctor = crate::core::backup_doctor::BackupDoctor::for_test_with_restore(
        backup_dir.path(),
        "missing-backup.sh",
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
