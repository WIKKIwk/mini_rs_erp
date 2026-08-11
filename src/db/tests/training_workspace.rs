use std::str::FromStr;

use sqlx::postgres::PgConnectOptions;

use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_training_workspace::PostgresTrainingWorkspaceStore;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_training_workspace"]
async fn deleting_training_order_removes_only_its_queue_states() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_training_workspace";
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

    let test_options = PgConnectOptions::from_str(&admin_url)
        .expect("parse admin db url")
        .database(db_name);
    let pool = sqlx::PgPool::connect_with(test_options)
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    sqlx::query(
        "INSERT INTO mini_training_production_maps (id, order_number, map_json)
         VALUES ($1, $2, $3)",
    )
    .bind("training-1001")
    .bind("1001")
    .bind(serde_json::json!({"id": "training-1001"}))
    .execute(&pool)
    .await
    .expect("insert training map");
    sqlx::query(
        "INSERT INTO mini_training_queue_states (apparatus, order_id, state)
         VALUES ('Flexo', 'training-1001', 'paused'),
                ('Laminatsiya', 'training-1001', 'pending'),
                ('Flexo', 'training-keep', 'pending')",
    )
    .execute(&pool)
    .await
    .expect("insert queue states");
    sqlx::query(
        "INSERT INTO mini_training_queue_events
            (event_id, apparatus, order_id, action, from_state, to_state,
             actor_ref, actor_display_name)
         VALUES
            ('training-event-delete', 'Flexo', 'training-1001', 'complete', 'pending', 'completed', 'worker-1', 'Worker 1'),
            ('training-event-keep', 'Flexo', 'training-keep', 'start', 'pending', 'in_progress', 'worker-2', 'Worker 2')",
    )
    .execute(&pool)
    .await
    .expect("insert queue events");

    PostgresTrainingWorkspaceStore::new(pool.clone())
        .delete_order("training-1001")
        .await
        .expect("delete training order");

    let deleted_order_states: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_training_queue_states WHERE order_id = $1")
            .bind("training-1001")
            .fetch_one(&pool)
            .await
            .expect("count deleted order states");
    let unrelated_states: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_training_queue_states WHERE order_id = $1")
            .bind("training-keep")
            .fetch_one(&pool)
            .await
            .expect("count unrelated states");
    let deleted_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_training_queue_events WHERE order_id = $1")
            .bind("training-1001")
            .fetch_one(&pool)
            .await
            .expect("count deleted queue events");
    let unrelated_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_training_queue_events WHERE order_id = $1")
            .bind("training-keep")
            .fetch_one(&pool)
            .await
            .expect("count unrelated queue events");
    let deleted_map: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_training_production_maps WHERE id = $1")
            .bind("training-1001")
            .fetch_one(&pool)
            .await
            .expect("count deleted map");
    assert_eq!(deleted_order_states, 0);
    assert_eq!(unrelated_states, 1);
    assert_eq!(deleted_events, 0);
    assert_eq!(unrelated_events, 1);
    assert_eq!(deleted_map, 0);

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
