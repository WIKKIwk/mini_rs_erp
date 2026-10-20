//! Commit-only invalidation for production readers on other server processes.
//! PostgreSQL notifications are hints, not a durable event log: LISTEN startup
//! and every reconnect invalidate all cached views to cover an offline gap.
use crate::core::apparatus_standard::CanonicalApparatusService;
use crate::core::production_map::ProductionMapService;
use serde::Deserialize;
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use std::time::Duration;

const CHANNEL: &str = "mini_production_live";
const COALESCE: Duration = Duration::from_millis(10);
const MAX_BATCH: usize = 128;

pub(crate) fn start_listener(
    pool: PgPool,
    service: ProductionMapService,
    apparatus: CanonicalApparatusService,
) -> Option<tokio::task::JoinHandle<()>> {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return None;
    };
    Some(runtime.spawn(async move {
        loop {
            if pool.is_closed() {
                return;
            }
            if let Err(error) = listen_once(&pool, &service, &apparatus).await {
                if pool.is_closed() {
                    return;
                }
                tracing::warn!(%error, "production postgres listener disconnected");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }))
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PreflightChange {
    kind: String,
    order_id: String,
    apparatus: String,
    #[serde(default)]
    event_id: Option<String>,
}

#[derive(Default)]
struct Pending {
    generic: bool,
    apparatus: bool,
    preflight: Vec<PreflightChange>,
}

impl Pending {
    fn add(&mut self, payload: &str) {
        if payload == "apparatus" {
            self.apparatus = true;
            self.generic = true;
            return;
        }
        match serde_json::from_str::<PreflightChange>(payload) {
            Ok(change)
                if change.kind == "preflight"
                    && !change.order_id.is_empty()
                    && crate::core::production_map::queue_state::is_canonical_apparatus_id(
                        &change.apparatus,
                    )
                    && change.event_id.as_ref().is_none_or(|id| {
                        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
                    }) =>
            {
                if !self.preflight.contains(&change) {
                    self.preflight.push(change);
                }
            }
            _ => self.generic = true,
        }
    }

    async fn publish(self, service: &ProductionMapService, apparatus: &CanonicalApparatusService) {
        if self.apparatus {
            apparatus.invalidate_runtime_caches().await;
        }
        let changes = self
            .preflight
            .iter()
            .map(|change| {
                (
                    change.event_id.as_deref(),
                    change.order_id.as_str(),
                    change.apparatus.as_str(),
                )
            })
            .collect::<Vec<_>>();
        service.notify_print_preflight_batch(&changes, self.generic);
    }
}

async fn invalidate_after_gap(
    service: &ProductionMapService,
    apparatus: &CanonicalApparatusService,
) {
    apparatus.invalidate_runtime_caches().await;
    service.notify_live();
}

async fn listen_once(
    pool: &PgPool,
    service: &ProductionMapService,
    apparatus: &CanonicalApparatusService,
) -> Result<(), sqlx::Error> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen(CHANNEL).await?;
    invalidate_after_gap(service, apparatus).await;
    loop {
        // recv() would hide an automatic reconnection and therefore a missed
        // notification gap. try_recv() explicitly reports that boundary.
        let Some(notification) = listener.try_recv().await? else {
            invalidate_after_gap(service, apparatus).await;
            continue;
        };
        let mut pending = Pending::default();
        pending.add(notification.payload());
        let deadline = tokio::time::Instant::now() + COALESCE;
        for _ in 1..MAX_BATCH {
            match tokio::time::timeout_at(deadline, listener.try_recv()).await {
                Ok(Ok(Some(notification))) => pending.add(notification.payload()),
                Ok(Ok(None)) => {
                    pending.generic = true;
                    pending.apparatus = true;
                    break;
                }
                Ok(Err(error)) => return Err(error),
                Err(_) => break,
            }
        }
        pending.publish(service, apparatus).await;
    }
}

#[cfg(test)]
mod tests {
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
            "CREATE TABLE mini_queue_states (id integer); CREATE TABLE mini_orders (id integer PRIMARY KEY);",
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
             UPDATE mini_orders SET id = id;",
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
}
