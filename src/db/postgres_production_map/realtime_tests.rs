use super::*;

fn preflight(order: &str) -> String {
    serde_json::json!({
        "kind":"preflight", "order_id":order,
        "apparatus":"apparatus:default:bosma_9"
    })
    .to_string()
}

#[test]
fn typed_notifications_retain_identity_until_publication() {
    let mut pending = Pending::default();
    pending.add(&preflight("a"));
    pending.add(&preflight("a"));
    assert!(!pending.generic);
    pending.add(&preflight("b"));
    assert!(!pending.generic);
    assert_eq!(pending.preflight.len(), 2);
    let mut pending = Pending::default();
    pending.add(&preflight("a"));
    pending.add("state");
    assert!(pending.generic);
    pending.add("apparatus");
    assert!(pending.apparatus);
}

#[tokio::test]
async fn gaps_and_mixed_changes_always_invalidate_authoritative_revision() {
    let store =
        std::sync::Arc::new(crate::core::production_map::MemoryProductionMapStore::new());
    let service = ProductionMapService::new_for_test(store);
    let apparatus = CanonicalApparatusService::memory();
    let mut events = service.subscribe_live();
    invalidate_after_gap(&service, &apparatus).await;
    assert_eq!(service.snapshot_revision(), 1);
    assert_eq!(
        events.recv().await.unwrap(),
        crate::core::production_map::ProductionMapLiveEvent::Invalidate
    );
    let mut pending = Pending::default();
    pending.add(&preflight("a"));
    pending.publish(&service, &apparatus).await;
    assert!(matches!(
        events.recv().await.unwrap(),
        crate::core::production_map::ProductionMapLiveEvent::PrintPreflight {
            revision: 2, order_id, ..
        } if order_id == "a"
    ));
}

#[tokio::test]
async fn sequence_local_and_remote_arrival_order_preserves_one_identical_delta() {
    use crate::core::production_map::{MemoryProductionMapStore, ProductionMapLiveEvent};
    let apparatus = CanonicalApparatusService::memory();
    for local_first in [true, false] {
        let service = ProductionMapService::new_for_test(std::sync::Arc::new(
            MemoryProductionMapStore::new(),
        ));
        let mut events = service.subscribe_live();
        let payload = serde_json::json!({
            "kind":"sequence", "event_id":"a".repeat(64),
            "apparatus":"apparatus:default:bosma_8",
            "base_revision":3, "revision":4,
            "version":"b".repeat(64),
            "ops":[{"type":"move","id":"a","before_id":"b","after_id":null,
                    "base_version":"c".repeat(64),"version":"b".repeat(64)}],
        })
        .to_string();
        let change: SequenceChange = serde_json::from_str(&payload).unwrap();
        if local_first {
            change.publish(&service);
        }
        let mut pending = Pending::default();
        pending.add(&payload);
        pending.add(&payload);
        assert_eq!(pending.sequence.len(), 1);
        pending.publish(&service, &apparatus).await;
        if !local_first {
            change.publish(&service);
        }
        let ProductionMapLiveEvent::Delta(delta) = events.try_recv().unwrap() else {
            panic!("listener-first must retain the legacy Delta");
        };
        assert_eq!(delta.epoch, service.snapshot_epoch());
        assert_eq!(delta.revision, 4);
        assert_eq!(delta.ops, change.ops);
        assert_eq!(service.snapshot_revision(), 1);
        assert!(events.try_recv().is_err());

        let mut pending = Pending::default();
        pending.add(&payload); // acknowledged typed event does not hide an external write
        pending.add("state");
        pending.publish(&service, &apparatus).await;
        assert_eq!(
            events.try_recv().unwrap(),
            ProductionMapLiveEvent::Invalidate
        );
        assert_eq!(service.snapshot_revision(), 2);
    }
}

#[tokio::test]
#[ignore = "requires PRODUCTION_LIVE_TEST_DATABASE_URL on an isolated bench_order_stream_ clone"]
async fn postgres_commit_rollback_two_readers_and_reconnect() {
    use crate::core::production_map::{MemoryProductionMapStore, ProductionMapLiveEvent};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    let url = std::env::var("PRODUCTION_LIVE_TEST_DATABASE_URL").expect("explicit clone URL");
    let parsed = reqwest::Url::parse(&url).unwrap();
    assert!(
        parsed
            .path()
            .trim_start_matches('/')
            .starts_with("bench_order_stream_")
    );
    assert!(matches!(
        parsed.host_str(),
        Some("127.0.0.1" | "localhost" | "::1")
    ));
    let schema = format!("live_test_{:016x}", rand::random::<u64>());
    let options = PgConnectOptions::from_str(&url).unwrap();
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options.clone())
        .await
        .unwrap();
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = options.options([("search_path", schema.as_str())]);
    let writer = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options.clone())
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE mini_queue_states (id integer);
             CREATE TABLE mini_orders (id integer PRIMARY KEY);
             CREATE TABLE mini_queue_sequences (id integer);
             CREATE TABLE mini_queue_events (id integer);
             CREATE TABLE mini_queue_reorder_commands (id integer);",
    )
    .execute(&writer)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/postgres/0156_production_live_invalidation.sql"
    ))
    .execute(&writer)
    .await
    .unwrap();
    let first_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone().application_name(&format!("{schema}_first")))
        .await
        .unwrap();
    let second_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.application_name(&format!("{schema}_second")))
        .await
        .unwrap();
    let first = ProductionMapService::new_for_test(std::sync::Arc::new(
        MemoryProductionMapStore::new(),
    ));
    let second = ProductionMapService::new_for_test(std::sync::Arc::new(
        MemoryProductionMapStore::new(),
    ));
    let mut first_events = first.subscribe_live();
    let mut second_events = second.subscribe_live();
    let first_task = start_listener(
        first_pool.clone(),
        first.clone(),
        CanonicalApparatusService::memory(),
    )
    .unwrap();
    let second_task = start_listener(
        second_pool.clone(),
        second.clone(),
        CanonicalApparatusService::memory(),
    )
    .unwrap();
    for events in [&mut first_events, &mut second_events] {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
                .unwrap(),
            ProductionMapLiveEvent::Invalidate
        );
    }
    let mut tx = writer.begin().await.unwrap();
    sqlx::query("INSERT INTO mini_orders VALUES (1)")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), first_events.recv())
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), second_events.recv())
            .await
            .is_err()
    );
    let mut tx = writer.begin().await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO mini_orders VALUES (1); INSERT INTO mini_queue_states VALUES (1)",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    for events in [&mut first_events, &mut second_events] {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
                .unwrap(),
            ProductionMapLiveEvent::Invalidate
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), events.recv())
                .await
                .is_err()
        );
    }
    // Page reads/backfills use conditional writes. Neither an empty
    // statement nor an unchanged UPSERT may invalidate its own readers.
    sqlx::raw_sql(
        "UPDATE mini_orders SET id = id WHERE id = -1;
             DELETE FROM mini_orders WHERE id = -1;
             INSERT INTO mini_orders VALUES (1) ON CONFLICT DO NOTHING;
             INSERT INTO mini_orders VALUES (1)
                ON CONFLICT (id) DO UPDATE SET id = EXCLUDED.id;
             UPDATE mini_orders SET id = id;
             INSERT INTO mini_queue_reorder_commands VALUES (1);",
    )
    .execute(&writer)
    .await
    .unwrap();
    for events in [&mut first_events, &mut second_events] {
        assert!(
            tokio::time::timeout(Duration::from_millis(100), events.recv())
                .await
                .is_err(),
            "no-op backfills must not invalidate a live read"
        );
    }
    // Ordinary external sequence writes remain authoritative. The narrow
    // transaction marker must never suppress unrelated projection inputs.
    for sql in [
        "INSERT INTO mini_queue_sequences VALUES (1)",
        "BEGIN; SELECT set_config('mini.production_live','sequence_commit_deferred',true);
             INSERT INTO mini_queue_sequences VALUES (2);
             INSERT INTO mini_queue_events VALUES (2);
             INSERT INTO mini_orders VALUES (3); COMMIT",
    ] {
        sqlx::raw_sql(sql).execute(&writer).await.unwrap();
        for events in [&mut first_events, &mut second_events] {
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(5), events.recv())
                    .await
                    .unwrap()
                    .unwrap(),
                ProductionMapLiveEvent::Invalidate
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(100), events.recv())
                    .await
                    .is_err()
            );
        }
    }
    let mut tx = writer.begin().await.unwrap();
    const EVENT_ID: &str = "1234567890abcdef1234567890abcdef";
    let mut tracked: serde_json::Value =
        serde_json::from_str(&preflight("colour-order")).unwrap();
    tracked["event_id"] = serde_json::json!(EVENT_ID);
    sqlx::query("SELECT set_config('mini.production_live', $1, true)")
        .bind(tracked.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::raw_sql(
        "INSERT INTO mini_orders VALUES (2); INSERT INTO mini_queue_states VALUES (2)",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    // The local service and LISTEN share this exact committed ID. If
    // either wins, the other must leave the global revision unchanged.
    first.notify_print_preflight_once(
        Some(EVENT_ID),
        "colour-order",
        "apparatus:default:bosma_9",
    );
    for events in [&mut first_events, &mut second_events] {
        assert!(
            matches!(tokio::time::timeout(Duration::from_secs(5), events.recv()).await.unwrap().unwrap(),
            ProductionMapLiveEvent::PrintPreflight {order_id, ..} if order_id == "colour-order")
        );
    }
    assert!(!second.notify_print_preflight_once(
        Some(EVENT_ID),
        "colour-order",
        "apparatus:default:bosma_9",
    ));
    for events in [&mut first_events, &mut second_events] {
        assert!(
            tokio::time::timeout(Duration::from_millis(100), events.recv())
                .await
                .is_err(),
            "local and remote publishers must deliver the committed event once"
        );
    }
    sqlx::query("TRUNCATE mini_orders")
        .execute(&writer)
        .await
        .unwrap();
    for events in [&mut first_events, &mut second_events] {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
                .unwrap(),
            ProductionMapLiveEvent::Invalidate
        );
    }
    // A terminated LISTEN session must invalidate even without another
    // notification: writes committed during disconnection may be missed.
    sqlx::query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = $1",
    )
    .bind(format!("{schema}_first"))
    .execute(&admin)
    .await
    .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(8), first_events.recv())
            .await
            .unwrap()
            .unwrap(),
        ProductionMapLiveEvent::Invalidate
    );
    first_task.abort();
    second_task.abort();
    let _ = first_task.await;
    let _ = second_task.await;
    first_pool.close().await;
    second_pool.close().await;
    writer.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
