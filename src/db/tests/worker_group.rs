use std::sync::Arc;

use sqlx::postgres::PgConnectOptions;

use crate::core::worker_groups::{WorkerGroupError, WorkerGroupService, WorkerGroupUpsert};
use crate::core::workers::{WorkerService, WorkerUpsert};
use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_worker::PostgresWorkerStore;
use crate::db::postgres_worker_group::PostgresWorkerGroupStore;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_worker_group_concurrency"]
async fn postgres_worker_group_mutations_are_serialized_with_worker_deactivation() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".to_string());
    let db_name = "mini_rs_erp_test_worker_group_concurrency";
    assert!(db_name.starts_with("mini_rs_erp_test_"));

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
        .expect("valid admin database url")
        .database(db_name);
    let pool = sqlx::PgPool::connect_with(test_options)
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migrations");

    let groups = Arc::new(WorkerGroupService::new(Arc::new(
        PostgresWorkerGroupStore::new(pool.clone()),
    )));
    let workers = Arc::new(WorkerService::new(Arc::new(PostgresWorkerStore::new(
        pool.clone(),
    ))));
    for (id, name, phone) in [
        ("worker-a", "Worker A", "+998901000001"),
        ("worker-b", "Worker B", "+998901000002"),
    ] {
        workers
            .upsert_worker(WorkerUpsert {
                id: id.to_string(),
                name: name.to_string(),
                phone: phone.to_string(),
                level: "1 - darajali".to_string(),
            })
            .await
            .expect("seed worker");
    }
    for (group_code, worker_id) in [("A guruh", "worker-a"), ("B guruh", "worker-b")] {
        groups
            .upsert_group(group_input(group_code, worker_id, "kunduz"))
            .await
            .expect("seed group");
    }

    let edit_a = groups.upsert_group(group_edit("A guruh", "worker-a", "tungi-a"));
    let edit_b = groups.upsert_group(group_edit("B guruh", "worker-b", "tungi-b"));
    let (saved_a, saved_b) = tokio::join!(edit_a, edit_b);
    saved_a.expect("edit A group");
    saved_b.expect("edit B group");
    let saved = groups
        .worker_groups(Some("Laminatsiya 1"))
        .await
        .expect("load edited groups");
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0].shift, "tungi-a");
    assert_eq!(saved[1].shift, "tungi-b");

    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let update = {
        let groups = groups.clone();
        let barrier = barrier.clone();
        async move {
            barrier.wait().await;
            groups
                .upsert_group(group_edit("A guruh", "worker-a", "race-update"))
                .await
        }
    };
    let deactivate = {
        let workers = workers.clone();
        let barrier = barrier.clone();
        async move {
            barrier.wait().await;
            workers.deactivate_worker("worker-a").await
        }
    };
    let (update_result, deactivate_result) = tokio::join!(update, deactivate);
    deactivate_result.expect("deactivate worker");
    assert!(matches!(
        update_result,
        Ok(_) | Err(WorkerGroupError::WorkerNotFound)
    ));

    let saved = groups
        .worker_groups(Some("Laminatsiya 1"))
        .await
        .expect("load groups after race");
    assert!(saved.iter().all(|group| {
        group
            .worker_ids
            .iter()
            .all(|id| !id.eq_ignore_ascii_case("worker-a"))
    }));
    let worker_active: bool =
        sqlx::query_scalar("SELECT active FROM mini_workers WHERE id = 'worker-a'")
            .fetch_one(&pool)
            .await
            .expect("worker status");
    assert!(!worker_active);
    assert_eq!(
        groups
            .upsert_group(group_edit("A guruh", "worker-a", "inactive-update"))
            .await,
        Err(WorkerGroupError::WorkerNotFound)
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

fn group_input(group_code: &str, worker_id: &str, shift: &str) -> WorkerGroupUpsert {
    WorkerGroupUpsert {
        apparatus: "Laminatsiya 1".to_string(),
        group_code: group_code.to_string(),
        shift: shift.to_string(),
        worker_ids: vec![worker_id.to_string()],
        ..WorkerGroupUpsert::default()
    }
}

fn group_edit(group_code: &str, worker_id: &str, shift: &str) -> WorkerGroupUpsert {
    WorkerGroupUpsert {
        previous_apparatus: Some("Laminatsiya 1".to_string()),
        previous_group_code: Some(group_code.to_string()),
        ..group_input(group_code, worker_id, shift)
    }
}
