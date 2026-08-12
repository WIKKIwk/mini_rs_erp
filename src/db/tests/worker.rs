use sqlx::postgres::PgConnectOptions;

use crate::core::workers::{Worker, WorkerStorePort};
use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_worker::PostgresWorkerStore;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_worker_pagination"]
async fn postgres_worker_pagination_is_stable_for_duplicate_names() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_worker_pagination";
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

    let test_options = admin_url
        .parse::<PgConnectOptions>()
        .expect("admin database options")
        .database(db_name);
    let pool = sqlx::PgPool::connect_with(test_options)
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migrations");

    let store = PostgresWorkerStore::new(pool.clone());
    for (id, name, phone) in [
        ("worker-page-c", "Pagination Ali", "+998901110003"),
        ("worker-page-a", "PAGINATION ALI", "+998901110001"),
        ("worker-page-d", "pagination ali", "+998901110004"),
        ("worker-page-b", "Pagination Ali", "+998901110002"),
    ] {
        store
            .upsert_worker(worker(id, name, phone))
            .await
            .expect("insert worker");
    }

    let first_prefix = store
        .workers("pagination ali", 3)
        .await
        .expect("first page prefix");
    let second_prefix = store
        .workers("pagination ali", 5)
        .await
        .expect("second page prefix");
    let ids = first_prefix
        .into_iter()
        .take(2)
        .chain(second_prefix.into_iter().skip(2).take(2))
        .map(|worker| worker.id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "worker-page-a",
            "worker-page-b",
            "worker-page-c",
            "worker-page-d",
        ]
    );

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

fn worker(id: &str, name: &str, phone: &str) -> Worker {
    Worker {
        id: id.to_string(),
        name: name.to_string(),
        phone: phone.to_string(),
        level: "Master".to_string(),
    }
}
