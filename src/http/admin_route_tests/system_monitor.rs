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
    let backup_file = backup_dir.path().join("mini_rs_erp_20260624_180448.dump");
    std::fs::write(&backup_file, b"backup-bytes").expect("write backup");
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
    assert!(
        body["backups"]["latest"]["age_seconds"]
            .as_i64()
            .unwrap_or(-1)
            >= 0
    );

    assert!(body["database"]["status"].as_str().unwrap_or("").len() > 0);
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
