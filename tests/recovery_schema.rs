//! Called by the Python recovery suite against its disposable PostgreSQL cluster.
use std::path::PathBuf;
use std::process::Command;

use base64::Engine;
use mini_rs_erp::core::auth::code_vault::CodeCipher;
use mini_rs_erp::core::auth::password::{hash_password_sync, verify_password};
use mini_rs_erp::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    canonical_apparatus_service,
};
use sqlx::PgPool;

#[tokio::test]
#[ignore = "requires the isolated cluster created by tools/db/test_recovery_bundle.py"]
async fn current_schema_survives_full_recovery() {
    let source = std::env::var("MINI_ERP_RECOVERY_TEST_SOURCE").unwrap();
    let target = std::env::var("MINI_ERP_RECOVERY_TEST_TARGET").unwrap();
    let runtime = PathBuf::from(std::env::var_os("MINI_ERP_RECOVERY_TEST_RUNTIME").unwrap());
    let pg_dump = std::env::var_os("MINI_ERP_RECOVERY_TEST_PG_DUMP").unwrap();
    let source_pool = PgPool::connect(&source).await.unwrap();
    sqlx::raw_sql(
        "DROP SCHEMA public CASCADE; CREATE SCHEMA public;
         GRANT USAGE ON SCHEMA public TO PUBLIC;
         CREATE ROLE mini_rs_erp LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;",
    )
    .execute(&source_pool)
    .await
    .unwrap();
    apply_postgres_migrations_through_version(&source_pool, "0121")
        .await
        .unwrap();
    canonical_apparatus_service(source_pool.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    apply_foundation_migration(&source_pool).await.unwrap();
    let code = "401234567890";
    let hash = hash_password_sync(code).unwrap();
    let key = std::fs::read_to_string(runtime.join("data/access-code.key")).unwrap();
    let key = base64::engine::general_purpose::STANDARD
        .decode(key.trim())
        .unwrap();
    let encrypted = CodeCipher::from_key(&key)
        .unwrap()
        .encrypt("recovery-fixture-worker", &hash, code)
        .unwrap();
    sqlx::query("INSERT INTO mini_auth_accounts(principal_ref,credential_hash) VALUES($1,$2)")
        .bind("recovery-fixture-worker")
        .bind(&hash)
        .execute(&source_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO mini_auth_code_vault(principal_ref,credential_hash,encrypted_code) VALUES($1,$2,$3)")
        .bind("recovery-fixture-worker").bind(&hash).bind(encrypted).execute(&source_pool).await.unwrap();

    let dump = runtime.parent().unwrap().join("current-schema.dump");
    let output = Command::new(pg_dump)
        .args(["-w", "-Fc", "--compress=0", "--file"])
        .arg(&dump)
        .arg(&source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let archive = runtime
        .parent()
        .unwrap()
        .join("current.accord-backup.tar.gz");
    let recovery = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/db/recovery_bundle.py");
    let output = Command::new("python3")
        .env("MINI_ERP_DATABASE_URL", &source)
        .arg(&recovery)
        .arg("create")
        .arg("--runtime")
        .arg(&runtime)
        .arg("--dump")
        .arg(&dump)
        .arg("--output")
        .arg(&archive)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let installed = runtime.parent().unwrap().join("current-restored");
    let output = Command::new("python3")
        .env("MINI_ERP_RESTORE_DATABASE_URL", &target)
        .arg(&recovery)
        .arg("restore")
        .arg(&archive)
        .arg("--target")
        .arg(&installed)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let target_pool = PgPool::connect(&target).await.unwrap();
    let recovered_env: serde_json::Value = serde_json::from_slice(
        &std::fs::read(installed.join("recovery-environment.json")).unwrap(),
    )
    .unwrap();
    let restored_key = base64::engine::general_purpose::STANDARD
        .decode(recovered_env["MINI_ERP_ACCESS_CODE_KEY"].as_str().unwrap())
        .unwrap();
    let (restored_hash, ciphertext): (String, String) = sqlx::query_as(
        "SELECT credential_hash,encrypted_code FROM mini_auth_code_vault
         WHERE principal_ref='recovery-fixture-worker'",
    )
    .fetch_one(&target_pool)
    .await
    .unwrap();
    assert!(verify_password(&restored_hash, code).await.unwrap());
    assert_eq!(
        CodeCipher::from_key(&restored_key)
            .unwrap()
            .decrypt("recovery-fixture-worker", &restored_hash, &ciphertext)
            .unwrap(),
        code,
    );
    for query in [
        "SELECT count(*)::text FROM mini_schema_migrations",
        "SELECT count(*)::text FROM mini_apparatus",
        "SELECT has_table_privilege('mini_rs_erp','mini_auth_accounts','DELETE')::text",
        "SELECT has_table_privilege('mini_rs_erp','mini_auth_cutovers','UPDATE')::text",
        "SELECT has_table_privilege('mini_rs_erp','mini_qolip_sets','UPDATE')::text",
        "SELECT has_column_privilege('mini_rs_erp','mini_qolip_sets','display_code','UPDATE')::text",
        "SELECT pg_get_functiondef('mini_reset_order_number_sequence()'::regprocedure)",
        "SELECT pg_get_functiondef('mini_qolip_register_set()'::regprocedure)",
    ] {
        let before: String = sqlx::query_scalar(query)
            .fetch_one(&source_pool)
            .await
            .unwrap();
        let after: String = sqlx::query_scalar(query)
            .fetch_one(&target_pool)
            .await
            .unwrap();
        assert_eq!(before, after, "{query}");
    }
    source_pool.close().await;
    target_pool.close().await;
}
