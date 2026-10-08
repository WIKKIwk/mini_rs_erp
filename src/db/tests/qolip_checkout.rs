use crate::core::production_map::{OrderRunSession, OrderRunStatus, ProductionMapStorePort};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    canonical_apparatus_service, postgres_test_database_options,
};
use crate::db::postgres_production_map::PostgresProductionMapStore;

struct Fixture {
    pool: sqlx::PgPool,
    admin_url: String,
    database: String,
    store: PostgresProductionMapStore,
}

impl Fixture {
    async fn new() -> Self {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let database = format!("qolip_checkout_freeze_{:032x}", rand::random::<u128>());
        let admin = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
        sqlx::query(&format!(r#"CREATE DATABASE "{database}""#))
            .execute(&admin)
            .await
            .expect("create test db");
        admin.close().await;
        let pool =
            sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &database))
                .await
                .expect("test db");
        apply_postgres_migrations_through_version(&pool, "0121")
            .await
            .unwrap();
        canonical_apparatus_service(pool.clone())
            .bootstrap_factory_defaults()
            .await
            .unwrap();
        apply_foundation_migration(&pool)
            .await
            .expect("apply migrations");
        sqlx::query(
            "INSERT INTO mini_production_maps (id, product_code, title, map_json)
             VALUES ('order-qolip-completion', 'ITEM-1', 'Test product', '{}'::jsonb)",
        )
        .execute(&pool)
        .await
        .expect("insert order");
        let store = PostgresProductionMapStore::new(pool.clone());
        store
            .put_order_run_session(session(OrderRunStatus::Active))
            .await
            .expect("active session");
        Self {
            pool,
            admin_url,
            database,
            store,
        }
    }

    async fn cleanup(self) {
        self.pool.close().await;
        let admin = sqlx::PgPool::connect(&self.admin_url)
            .await
            .expect("admin db");
        sqlx::query(&format!(
            r#"DROP DATABASE "{}" WITH (FORCE)"#,
            self.database
        ))
        .execute(&admin)
        .await
        .expect("drop test db");
        admin.close().await;
    }
}

#[tokio::test]
async fn stopped_qolip_session_returns_only_its_workers_physical_checkouts_once() {
    for status in [OrderRunStatus::Completed, OrderRunStatus::Frozen] {
        let f = Fixture::new().await;
        insert_open_checkout(&f.pool, "checkout-owned", "worker-1", "Q-SESSION").await;
        insert_open_checkout(&f.pool, "checkout-other-worker", "worker-2", "Q-SESSION").await;
        insert_open_checkout(&f.pool, "checkout-other-qolip", "worker-1", "Q-OTHER").await;
        // A freeze request leaves the active session and its physical checkouts held.
        assert_eq!(checkout_status(&f.pool, "checkout-owned").await, "open");
        assert!(
            f.store
                .active_order_run_session_for_qolip("Q-SESSION")
                .await
                .unwrap()
                .is_some()
        );

        let mut stopped = session(status);
        stopped.release_frozen_qolips(); // Same normalized write as the queue boundary.
        f.store
            .put_order_run_session(stopped.clone())
            .await
            .expect("stop session");
        assert_eq!(checkout_status(&f.pool, "checkout-owned").await, "returned");
        assert_eq!(
            checkout_status(&f.pool, "checkout-other-worker").await,
            "open"
        );
        assert_eq!(
            checkout_status(&f.pool, "checkout-other-qolip").await,
            "open"
        );
        let location: (String, String, i32, String) = sqlx::query_as(
            "SELECT qolip_code, warehouse, quantity, location_label FROM mini_qolip_locations
             WHERE qolip_code = 'Q-SESSION'",
        )
        .fetch_one(&f.pool)
        .await
        .unwrap();
        assert_eq!(
            location,
            ("Q-SESSION".into(), "Qolip ombori".into(), 1, "A1".into())
        );
        let phantom: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM mini_qolip_locations WHERE qolip_code = 'Q-CATALOG-ONLY'",
        )
        .fetch_one(&f.pool)
        .await
        .unwrap();
        assert_eq!(
            phantom, 0,
            "lineage without a physical checkout never becomes stock"
        );
        assert!(
            f.store
                .active_order_run_session_for_qolip("Q-SESSION")
                .await
                .unwrap()
                .is_none()
        );
        let saved = f
            .store
            .order_run_session(&stopped.session_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            saved.payload_json["qolip_codes"],
            serde_json::json!(["Q-SESSION", "Q-CATALOG-ONLY"])
        );
        if status == OrderRunStatus::Frozen {
            assert_eq!(saved.payload_json["qolip_lock_owner"], false);
            assert!(saved.qolip_reacquisition_required());
            assert_eq!(saved.payload_json["qolip_returned_checkout_count"], 1);
            let resources =
                crate::core::production_map::ProductionQrSessionResources::for_session(&saved);
            assert!(!resources.qolip_available);
            assert!(resources.qolip_codes.is_empty());
        }
        f.store
            .put_order_run_session(stopped.clone())
            .await
            .expect("repeat stop");
        let quantity: i32 = sqlx::query_scalar(
            "SELECT quantity FROM mini_qolip_locations WHERE qolip_code = 'Q-SESSION'",
        )
        .fetch_one(&f.pool)
        .await
        .unwrap();
        assert_eq!(quantity, 1, "replays must not restore stock twice");
        if status == OrderRunStatus::Frozen {
            insert_open_checkout(&f.pool, "checkout-later", "worker-1", "Q-SESSION").await;
            // An old freeze write cannot return a checkout issued after the release.
            f.store
                .put_order_run_session(session(status))
                .await
                .unwrap();
            assert_eq!(checkout_status(&f.pool, "checkout-later").await, "open");
            let replay = f
                .store
                .order_run_session(&stopped.session_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(replay.payload_json["qolip_returned_checkout_count"], 1);
        }
        f.cleanup().await;
    }
}

#[tokio::test]
async fn frozen_qolip_return_failure_rolls_back_session_and_all_checkouts() {
    let f = Fixture::new().await;
    insert_open_checkout(&f.pool, "checkout-owned", "worker-1", "Q-SESSION").await;
    insert_open_checkout(&f.pool, "checkout-invalid", "worker-1", "Q-CATALOG-ONLY").await;
    sqlx::query(
        "INSERT INTO mini_qolip_locations (id, block, warehouse, item_code, item_name,
             qolip_code, size, quantity, row_letter, column_number, location_label)
         VALUES ('qolip:a:item_1:q_catalog_only:40:a:1', 'A', 'Qolip ombori', 'ITEM-1', 'Test product',
             'DIFFERENT-MOLD', 40, 1, 'A', 1, 'A1')",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    assert!(
        f.store
            .put_order_run_session(session(OrderRunStatus::Frozen))
            .await
            .is_err()
    );
    let saved = f
        .store
        .order_run_session("session-qolip-completion")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.status, OrderRunStatus::Active);
    assert_eq!(saved.payload_json["qolip_lock_owner"], true);
    assert_eq!(checkout_status(&f.pool, "checkout-owned").await, "open");
    assert_eq!(checkout_status(&f.pool, "checkout-invalid").await, "open");
    let locations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mini_qolip_locations")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(locations, 1, "partial stock restoration must roll back");
    f.cleanup().await;
}

#[tokio::test]
async fn frozen_qolip_reconciliation_repairs_legacy_owners_and_is_idempotent() {
    let f = Fixture::new().await;
    insert_open_checkout(&f.pool, "checkout-owned", "worker-1", "Q-SESSION").await;
    sqlx::query("UPDATE mini_order_run_sessions SET status = 'frozen'")
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(f.store.reconcile_frozen_qolip_returns().await.unwrap(), 1);
    assert_eq!(checkout_status(&f.pool, "checkout-owned").await, "returned");
    assert!(
        f.store
            .active_order_run_session_for_qolip("Q-SESSION")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.store.reconcile_frozen_qolip_returns().await.unwrap(), 0);
    let quantity: i32 = sqlx::query_scalar("SELECT quantity FROM mini_qolip_locations")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(quantity, 1);
    f.cleanup().await;
}

#[tokio::test]
async fn frozen_downstream_lineage_does_not_return_a_printers_checkout() {
    let f = Fixture::new().await;
    insert_open_checkout(&f.pool, "checkout-owned", "worker-1", "Q-SESSION").await;
    let mut frozen = session(OrderRunStatus::Frozen);
    frozen.payload_json["qolip_lock_owner"] = serde_json::json!(false);
    f.store.put_order_run_session(frozen).await.unwrap();
    assert_eq!(checkout_status(&f.pool, "checkout-owned").await, "open");
    let locations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mini_qolip_locations")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(locations, 0);
    f.cleanup().await;
}

fn session(status: OrderRunStatus) -> OrderRunSession {
    OrderRunSession {
        session_id: "session-qolip-completion".to_string(),
        apparatus: "apparatus:default:bosma_7".to_string(),
        order_id: "order-qolip-completion".to_string(),
        stage_node_id: "bosma_7".to_string(),
        status,
        worker_role: "bosmachi".to_string(),
        worker_ref: "worker-1".to_string(),
        worker_display_name: "Worker One".to_string(),
        started_at_unix: 1,
        updated_at_unix: 2,
        payload_json: serde_json::json!({
            "qolip_lock_owner": true,
            "qolip_code": "Q-SESSION",
            "qolip_codes": ["Q-SESSION", "Q-CATALOG-ONLY"],
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
    .bind(crate::core::qolip::normalize::qolip_location_id(
        "A",
        "ITEM-1",
        qolip_code,
        40,
        "A",
        Some(1),
    ))
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
