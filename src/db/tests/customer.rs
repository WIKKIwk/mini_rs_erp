use crate::core::admin::ports::AdminPortError;
use crate::core::workers::{Worker, WorkerError, WorkerStorePort};
use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_customer::PostgresCustomerStore;
use crate::db::postgres_worker::PostgresWorkerStore;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_phone_uniqueness"]
async fn postgres_normalized_phones_are_unique_under_database_control() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_phone_uniqueness";
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let test_url = format!("postgres://wikki@127.0.0.1:5432/{db_name}");
    let pool = sqlx::PgPool::connect(&test_url).await.expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migrations");

    let customers = PostgresCustomerStore::new(pool.clone());
    customers
        .create_customer("Customer A", "+998 90 123-45-67")
        .await
        .expect("create first customer");
    let duplicate_customer = customers
        .create_customer("Customer B", "998901234567")
        .await;
    assert!(matches!(
        duplicate_customer,
        Err(AdminPortError::InvalidInput(message)) if message == "phone already exists"
    ));

    let workers = PostgresWorkerStore::new(pool.clone());
    workers
        .upsert_worker(test_worker("worker-a", "Worker A", "+998 91 765-43-21"))
        .await
        .expect("create first worker");
    let duplicate_worker = workers
        .upsert_worker(test_worker("worker-b", "Worker B", "998917654321"))
        .await;
    assert!(matches!(duplicate_worker, Err(WorkerError::DuplicatePhone)));

    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("cleanup test db");
    admin_pool.close().await;
}

fn test_worker(id: &str, name: &str, phone: &str) -> Worker {
    Worker {
        id: id.to_string(),
        name: name.to_string(),
        phone: phone.to_string(),
        level: "Master".to_string(),
    }
}
