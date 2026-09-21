use super::*;
use crate::core::production_map::OrderEarlyClose;

// Connection-local objects only: never migrate or modify the application database.
#[tokio::test]
#[ignore = "requires a local PostgreSQL connection; uses only temporary objects"]
async fn postgres_early_close_is_atomic_durable_and_immutable() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let pool = sqlx::postgres::PgPoolOptions::new().max_connections(1).connect(&url).await.unwrap();
    sqlx::raw_sql("
        SET search_path TO pg_temp;
        CREATE TEMP TABLE mini_production_maps (
            id TEXT PRIMARY KEY, lifecycle_status TEXT DEFAULT 'in_progress', lifecycle_version BIGINT DEFAULT 1,
            completion_outcome TEXT DEFAULT '', closed_at TIMESTAMPTZ, production_completed_at TIMESTAMPTZ,
            lifecycle_changed_at TIMESTAMPTZ DEFAULT now()
        );
        CREATE TEMP TABLE mini_queue_sequences (apparatus TEXT PRIMARY KEY, order_ids JSONB, updated_at TIMESTAMPTZ);
        CREATE TEMP TABLE mini_apparatus (id TEXT PRIMARY KEY, name TEXT);
        CREATE TEMP TABLE mini_chat_messages (message_type TEXT);
        CREATE TEMP TABLE mini_production_order_lifecycle_events (
            event_id TEXT PRIMARY KEY, order_id TEXT, from_status TEXT, to_status TEXT, completion_outcome TEXT,
            actor_role TEXT, actor_ref TEXT, actor_display_name TEXT, source_event_id TEXT, reason TEXT,
            lifecycle_version BIGINT, created_at TIMESTAMPTZ
        );
        CREATE TEMP TABLE last_roll (id TEXT);
        CREATE TEMP TABLE mini_order_run_sessions (order_id TEXT, status TEXT);
        CREATE TEMP TABLE mini_queue_states (order_id TEXT, state TEXT);
        CREATE TEMP TABLE mini_print_preflight_holds (order_id TEXT, status TEXT);
        INSERT INTO mini_production_maps (id) VALUES ('zakaz-close');
        INSERT INTO mini_queue_sequences VALUES ('one', '[\"before\",\"zakaz-close\",\"after\"]', now());
        INSERT INTO mini_apparatus VALUES ('apparatus:test:one', 'One');
    ").execute(&pool).await.unwrap();
    for sql in [
        include_str!("../../../../migrations/postgres/0025_order_control_state.sql"),
        include_str!("../../../../migrations/postgres/0026_order_freeze_request_chat_cards.sql"),
    ] {
        sqlx::raw_sql(&sql.replace("CREATE TABLE IF NOT EXISTS", "CREATE TEMP TABLE IF NOT EXISTS"))
            .execute(&pool).await.unwrap();
    }
    sqlx::raw_sql("ALTER TABLE mini_order_freeze_requests ADD COLUMN canonical_target_apparatus_id TEXT")
        .execute(&pool).await.unwrap();
    // PostgreSQL never implicitly resolves functions from pg_temp.
    sqlx::raw_sql(&include_str!("../../../../migrations/postgres/0130_order_early_close.sql")
        .replace("EXECUTE FUNCTION mini_", "EXECUTE FUNCTION pg_temp.mini_"))
        .execute(&pool).await.unwrap();
    let actor = QueueActionActor { role: "admin".into(), ref_: "admin-1".into(), display_name: "Admin".into() };
    let mut record = OrderControlRecord {
        order_id: "zakaz-close".into(), state: OrderControlState::FreezeRequested, actor: actor.clone(),
        requested_at_unix: 100, frozen_at_unix: None,
        freeze_request: Some(OrderFreezeRequest {
            request_id: "freeze-close".into(), status: OrderFreezeRequestStatus::Pending,
            target_session_id: "session-1".into(), target_apparatus: "apparatus:test:one".into(),
            target_worker_role: "worker".into(), target_worker_ref: "worker-1".into(),
            target_worker_display_name: "Worker".into(), requested_at_unix: 100, transitioned_at_unix: 100,
        }),
        early_close: Some(OrderEarlyClose { comment: "Mijoz bekor qildi".into(), actor,
            requested_at_unix: 100, closed_at_unix: None }),
    };
    save_order_control_state(&pool, &record).await.unwrap();
    let pending = load_order_control_by_id(&pool, "zakaz-close").await.unwrap().unwrap();
    assert_eq!(pending, record);
    record.state = OrderControlState::Frozen;
    record.frozen_at_unix = Some(120);
    record.early_close.as_mut().unwrap().closed_at_unix = Some(120);
    record.freeze_request.as_mut().unwrap().status = OrderFreezeRequestStatus::Frozen;
    record.freeze_request.as_mut().unwrap().transitioned_at_unix = 120;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO last_roll VALUES ('roll-1')").execute(&mut *tx).await.unwrap();
    save_order_control_state_tx(&mut tx, &record).await.unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(load_order_control_by_id(&pool, "zakaz-close").await.unwrap().unwrap(), pending);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM last_roll").fetch_one(&pool).await.unwrap();
    assert_eq!(count, 0);
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO last_roll VALUES ('roll-1')").execute(&mut *tx).await.unwrap();
    save_order_control_state_tx(&mut tx, &record).await.unwrap();
    tx.commit().await.unwrap();
    save_order_control_state(&pool, &record).await.unwrap(); // retry cannot duplicate audit
    assert_eq!(load_order_control_by_id(&pool, "zakaz-close").await.unwrap().unwrap(), record);
    let status: (String, i64, bool) = sqlx::query_as(
        "SELECT lifecycle_status, lifecycle_version, production_completed_at IS NULL FROM mini_production_maps")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(status, ("cancelled".into(), 2, true));
    let audit: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT reason, actor_ref, to_status FROM mini_production_order_lifecycle_events")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(audit, vec![("Mijoz bekor qildi".into(), "admin-1".into(), "cancelled".into())]);
    let sequence: serde_json::Value = sqlx::query_scalar("SELECT order_ids FROM mini_queue_sequences")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(sequence, serde_json::json!(["before", "after"]));
    assert!(save_order_control_state(&pool, &pending).await.is_err(), "stale request cannot reopen");
    let mut tampered = record.clone();
    tampered.early_close.as_mut().unwrap().comment = "Changed reason".into();
    assert!(save_order_control_state(&pool, &tampered).await.is_err());
    assert_eq!(load_order_control_by_id(&pool, "zakaz-close").await.unwrap().unwrap(), record);
    // Untouched 0009 has no freeze request or worker session. Persist its
    // closure directly, but reject a stale preparation if work has started.
    sqlx::query("INSERT INTO mini_production_maps (id, lifecycle_status, lifecycle_version) VALUES ('zakaz-0009', 'released', 0)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_queue_sequences VALUES ('two', '[\"before\",\"zakaz-0009\",\"after\"]', now())")
        .execute(&pool).await.unwrap();
    let mut unstarted = record.clone();
    unstarted.order_id = "zakaz-0009".into();
    unstarted.freeze_request = None;
    sqlx::query("INSERT INTO mini_order_run_sessions VALUES ('zakaz-0009', 'active')")
        .execute(&pool).await.unwrap();
    assert_eq!(save_order_control_state(&pool, &unstarted).await,
        Err(ProductionMapError::OrderControlActionNotAllowed));
    assert!(load_order_control_by_id(&pool, "zakaz-0009").await.unwrap().is_none());
    sqlx::query("DELETE FROM mini_order_run_sessions WHERE order_id = 'zakaz-0009'")
        .execute(&pool).await.unwrap();
    save_order_control_state(&pool, &unstarted).await.unwrap();
    save_order_control_state(&pool, &unstarted).await.unwrap();
    assert_eq!(load_order_control_by_id(&pool, "zakaz-0009").await.unwrap().unwrap(), unstarted);
    let fresh: (String, i64, bool) = sqlx::query_as(
        "SELECT lifecycle_status, lifecycle_version, production_completed_at IS NULL FROM mini_production_maps WHERE id = 'zakaz-0009'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(fresh, ("cancelled".into(), 1, true));
    let audit_count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_production_order_lifecycle_events WHERE order_id = 'zakaz-0009' AND reason = 'Mijoz bekor qildi'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(audit_count, 1);
    let sequence: serde_json::Value = sqlx::query_scalar("SELECT order_ids FROM mini_queue_sequences WHERE apparatus = 'two'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(sequence, serde_json::json!(["before", "after"]));
    assert!(matches!(super::super::catalog_helpers::delete_map_by_id(&pool, "zakaz-0009").await,
        Err(ProductionMapError::OrderDeleteBlocked(_))));
    pool.close().await;
}
