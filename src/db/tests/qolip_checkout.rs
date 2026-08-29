use crate::core::production_map::{OrderRunSession, OrderRunStatus, ProductionMapStorePort};
use crate::core::qolip::{QolipError, QolipStorePort};
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_production_map::PostgresProductionMapStore;
use crate::db::postgres_qolip::PostgresQolipStore;

use super::seed_standard_canonical_apparatus;

#[tokio::test]
async fn completed_qolip_session_returns_only_its_workers_physical_checkout() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_qolip_checkout_completion";
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

    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, db_name))
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migrations");
    seed_standard_canonical_apparatus(&pool).await;

    insert_open_checkout(&pool, "checkout-owned", "worker-1", "Q-SESSION").await;
    insert_open_checkout(&pool, "checkout-other-worker", "worker-2", "Q-SESSION").await;
    insert_open_checkout(&pool, "checkout-other-qolip", "worker-1", "Q-OTHER").await;
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ('order-qolip-completion', 'ITEM-1', 'Test product', '{}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("insert qolip test order");
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
             session_id, apparatus, canonical_apparatus_id, order_id, status,
             worker_role, worker_ref, worker_display_name, payload_json
         ) VALUES (
             'session-qolip-completion', 'Bosma 7', 'apparatus:default:bosma_7',
             'order-qolip-completion', 'frozen', 'bosmachi', 'worker-1', 'Worker One',
             '{\"qolip_lock_owner\":true,\"qolip_code\":\"Q-SESSION\",\"qolip_codes\":[\"Q-SESSION\"]}'::jsonb
         )",
    )
    .execute(&pool)
    .await
    .expect("insert frozen qolip session");

    let frozen = session(OrderRunStatus::Frozen);
    let mut tx = pool.begin().await.expect("begin frozen transaction");
    let returned =
        crate::db::postgres_qolip::return_completed_session_checkouts_tx(&mut tx, &frozen)
            .await
            .expect("frozen session check");
    tx.commit().await.expect("commit frozen transaction");
    assert_eq!(returned, 0);
    assert_eq!(checkout_status(&pool, "checkout-owned").await, "open");
    let active_session = PostgresProductionMapStore::new(pool.clone())
        .active_order_run_session_for_qolip("Q-SESSION")
        .await
        .expect("load frozen qolip session")
        .expect("frozen session must keep the qolip blocked");
    assert_eq!(active_session.status, OrderRunStatus::Frozen);
    let manual_return = PostgresQolipStore::new(pool.clone())
        .return_checkout("checkout-owned", "A", Some(1))
        .await;
    assert_eq!(manual_return, Err(QolipError::QolipInUse));
    assert_eq!(checkout_status(&pool, "checkout-owned").await, "open");

    sqlx::query(
        "UPDATE mini_order_run_sessions
         SET status = 'completed'
         WHERE session_id = 'session-qolip-completion'",
    )
    .execute(&pool)
    .await
    .expect("complete frozen qolip session");

    let completed = session(OrderRunStatus::Completed);
    let mut tx = pool.begin().await.expect("begin completed transaction");
    let returned =
        crate::db::postgres_qolip::return_completed_session_checkouts_tx(&mut tx, &completed)
            .await
            .expect("completed session return");
    tx.commit().await.expect("commit completed transaction");
    assert_eq!(returned, 1);
    assert_eq!(checkout_status(&pool, "checkout-owned").await, "returned");
    assert_eq!(
        checkout_status(&pool, "checkout-other-worker").await,
        "open"
    );
    assert_eq!(checkout_status(&pool, "checkout-other-qolip").await, "open");

    let restored: (String, i32, String) = sqlx::query_as(
        "SELECT qolip_code, quantity, location_label
         FROM mini_qolip_locations
         WHERE qolip_code = 'Q-SESSION'",
    )
    .fetch_one(&pool)
    .await
    .expect("restored location");
    assert_eq!(restored, ("Q-SESSION".to_string(), 1, "A1".to_string()));

    let mut tx = pool.begin().await.expect("begin idempotency transaction");
    let returned_again =
        crate::db::postgres_qolip::return_completed_session_checkouts_tx(&mut tx, &completed)
            .await
            .expect("repeat completed session return");
    tx.commit().await.expect("commit idempotency transaction");
    assert_eq!(returned_again, 0);
    let restored_quantity: i32 = sqlx::query_scalar(
        "SELECT quantity FROM mini_qolip_locations WHERE qolip_code = 'Q-SESSION'",
    )
    .fetch_one(&pool)
    .await
    .expect("restored quantity");
    assert_eq!(restored_quantity, 1);

    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("reconnect admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db after test");
    admin_pool.close().await;
}

fn session(status: OrderRunStatus) -> OrderRunSession {
    OrderRunSession {
        session_id: "session-qolip-completion".to_string(),
        apparatus: "apparatus:default:bosma_7".to_string(),
        order_id: "order-qolip-completion".to_string(),
        status,
        worker_role: "bosmachi".to_string(),
        worker_ref: "worker-1".to_string(),
        worker_display_name: "Worker One".to_string(),
        started_at_unix: 1,
        updated_at_unix: 2,
        payload_json: serde_json::json!({
            "qolip_lock_owner": true,
            "qolip_code": "Q-SESSION",
            "qolip_codes": ["Q-SESSION"],
        }),
    }
}

async fn insert_open_checkout(pool: &sqlx::PgPool, id: &str, worker_ref: &str, qolip_code: &str) {
    sqlx::query(
        "INSERT INTO mini_qolip_checkouts (
             id, location_id, block, warehouse, item_code, item_name, qolip_code,
             size, quantity, row_letter, column_number, location_label,
             issued_to_ref, issued_to_name, status,
             issued_by_role, issued_by_ref, issued_by_name, payload_json
         ) VALUES (
             $1, $2, 'A', 'Qolip ombori', 'ITEM-1', 'Test product', $3,
             40, 1, 'A', 1, 'A1', $4, 'Test worker', 'open',
             'qolipchi', 'qolipchi-1', 'Qolipchi', '{}'::jsonb
         )",
    )
    .bind(id)
    .bind(format!("source-{id}"))
    .bind(qolip_code)
    .bind(worker_ref)
    .execute(pool)
    .await
    .expect("insert open checkout");
}

async fn checkout_status(pool: &sqlx::PgPool, id: &str) -> String {
    sqlx::query_scalar("SELECT status FROM mini_qolip_checkouts WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("checkout status")
}
