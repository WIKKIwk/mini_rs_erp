use std::process::Command;

use mini_rs_erp::core::auth::password::verify_password;
use mini_rs_erp::db::postgres::{
    apply_postgres_migrations_through_version, canonical_apparatus_service,
};
use sqlx::PgPool;

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database and ERP owner/runtime roles"]
async fn cli_import_cleanup_retry_and_stdin_reset() {
    let url = std::env::var("MINI_ERP_AUTH_CLI_TEST_DATABASE_URL").unwrap();
    let runtime_url = std::env::var("MINI_ERP_AUTH_CLI_TEST_RUNTIME_URL").unwrap();
    let pool = PgPool::connect(&url).await.unwrap();
    // Existing migrations 0122/0123 expect the factory apparatus catalog.
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    canonical_apparatus_service(pool.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join(".env"), format!(
        "MINI_ERP_DATABASE_URL={runtime_url}\nMINI_ERP_MIGRATION_DATABASE_URL={url}\nADMINKA_PHONE=+998901234500\nADMINKA_CODE=87654321\nKEEP_ME=preserved\n"
    )).unwrap();
    std::fs::create_dir(temp.path().join("data")).unwrap();
    let json_path = temp.path().join("data/mobile_admin_store.json");
    std::fs::write(&json_path, r#"{"states":{"CUST-01":{"custom_code":"301234567890"}},"customers":{"CUST-01":{"ref":"CUST-01","name":"Fixture","phone":"+998901234501"}}}"#).unwrap();
    let command = || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_mini_rs_auth_migrate"));
        cmd.env_clear().current_dir(temp.path());
        cmd
    };
    for _ in 0..2 {
        let result = command().output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!String::from_utf8_lossy(&result.stdout).contains("87654321"));
    }
    let env = std::fs::read_to_string(temp.path().join(".env")).unwrap();
    assert!(!env.contains("ADMINKA_CODE"));
    assert!(!env.contains("ADMINKA_PHONE"));
    assert!(env.contains("KEEP_ME=preserved"));
    assert!(
        !std::fs::read_to_string(&json_path)
            .unwrap()
            .contains("301234567890")
    );
    let hash: String = sqlx::query_scalar(
        "SELECT credential_hash FROM mini_auth_accounts WHERE principal_ref = 'admin'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(verify_password(&hash, "87654321").await.unwrap());
    let mut reset = command()
        .args(["--reset-admin", "--admin-code-stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    reset
        .stdin
        .take()
        .unwrap()
        .write_all(b"new-test-admin-code\n")
        .unwrap();
    let result = reset.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!String::from_utf8_lossy(&result.stdout).contains("new-test-admin-code"));
    let hash: String = sqlx::query_scalar(
        "SELECT credential_hash FROM mini_auth_accounts WHERE principal_ref = 'admin'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(verify_password(&hash, "new-test-admin-code").await.unwrap());
    assert!(!verify_password(&hash, "87654321").await.unwrap());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_schema_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 126);
    pool.close().await;
}
