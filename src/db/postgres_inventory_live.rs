//! Commit-only, cross-process invalidation for bounded inventory live views.
//! No row/customer data crosses NOTIFY. A reconnect invalidates every local
//! view because PostgreSQL notifications are deliberately not a durable log.
use crate::core::warehouse_events::WarehouseEventHub;
use sqlx::{PgPool, postgres::PgListener};
use std::time::Duration;

pub(crate) fn start_listener(
    pool: PgPool,
    hub: WarehouseEventHub,
) -> Option<tokio::task::JoinHandle<()>> {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return None;
    };
    Some(runtime.spawn(async move {
        // LISTEN holds its connection for the lifetime of the feed. Keep its
        // single connection outside the request pool so a small (even size 1)
        // runtime pool cannot be exhausted by an idle subscriber.
        let listener_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .max_lifetime(None)
            .idle_timeout(None)
            .acquire_timeout(Duration::from_secs(5))
            .connect_lazy_with((*pool.connect_options()).clone());
        while !pool.is_closed() {
            let result = tokio::select! {
                result = listen(&listener_pool, &hub) => result,
                _ = pool.close_event() => break,
            };
            if let Err(error) = result {
                if pool.is_closed() {
                    return;
                }
                tracing::warn!(%error, "inventory postgres listener disconnected");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        listener_pool.close().await;
    }))
}

async fn listen(pool: &PgPool, hub: &WarehouseEventHub) -> Result<(), sqlx::Error> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen("mini_inventory_live").await?;
    hub.invalidate_inventory();
    loop {
        // try_recv reports the reconnect boundary; recv would hide a gap.
        let notification = listener.try_recv().await?;
        if notification.is_none() {
            hub.invalidate_inventory();
            continue;
        }
        // Coalesce transaction bursts before broadcasting to subscribers.
        let deadline = tokio::time::Instant::now() + Duration::from_millis(50);
        for _ in 0..127 {
            match tokio::time::timeout_at(deadline, listener.try_recv()).await {
                Ok(Ok(Some(_))) => {}
                Ok(Ok(None)) => break,
                Ok(Err(error)) => return Err(error),
                Err(_) => break,
            }
        }
        hub.invalidate_inventory();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny isolated database exercises the actual migration and two server
    /// listeners without importing ERP customer data or apparatus fixtures.
    #[tokio::test]
    #[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated disposable database"]
    async fn inventory_live_postgres_commits_rollback_noop_and_two_listeners() {
        let url =
            std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").expect("explicit maintenance URL");
        let name = format!(
            "mini_rs_erp_test_inventory_live_{:016x}",
            rand::random::<u64>()
        );
        let admin = PgPool::connect(&url)
            .await
            .expect("test maintenance connection");
        sqlx::query(&format!("CREATE DATABASE {name}"))
            .execute(&admin)
            .await
            .unwrap();
        let options = crate::db::postgres::postgres_test_database_options(&url, &name);
        let writer = PgPool::connect_with(options.clone()).await.unwrap();
        sqlx::raw_sql(
            "CREATE TABLE mini_raw_material_stock (id integer PRIMARY KEY, qty integer);
            CREATE TABLE mini_factory_location_apparatus_links (id integer);
            CREATE TABLE mini_auth_accounts (id integer);
            CREATE TABLE mini_queue_states (id integer);",
        )
        .execute(&writer)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/postgres/0170_inventory_live.sql"
        ))
        .execute(&writer)
        .await
        .unwrap();
        let first_name = format!("{name}_first");
        let first_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone().application_name(&first_name))
            .await
            .unwrap();
        let second_pool = PgPool::connect_with(options).await.unwrap();
        let first = WarehouseEventHub::new();
        let second = WarehouseEventHub::new();
        let mut first_events = first.subscribe_inventory();
        let mut second_events = second.subscribe_inventory();
        let first_task = start_listener(first_pool.clone(), first).unwrap();
        let second_task = start_listener(second_pool.clone(), second).unwrap();
        for events in [&mut first_events, &mut second_events] {
            tokio::time::timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
                .unwrap();
        }
        tokio::time::timeout(
            Duration::from_secs(1),
            sqlx::query("SELECT 1").execute(&first_pool),
        )
        .await
        .expect("LISTEN must not occupy the only request-pool connection")
        .unwrap();
        let mut tx = writer.begin().await.unwrap();
        sqlx::query("INSERT INTO mini_raw_material_stock VALUES (1, 10)")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        for events in [&mut first_events, &mut second_events] {
            assert!(
                tokio::time::timeout(Duration::from_millis(150), events.recv())
                    .await
                    .is_err()
            );
        }
        let mut tx = writer.begin().await.unwrap();
        sqlx::query("INSERT INTO mini_raw_material_stock VALUES (1, 10), (2, 20)")
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        for events in [&mut first_events, &mut second_events] {
            tokio::time::timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(150), events.recv())
                    .await
                    .is_err(),
                "one coalesced invalidation per commit burst"
            );
        }
        sqlx::query("UPDATE mini_raw_material_stock SET qty = qty")
            .execute(&writer)
            .await
            .unwrap();
        sqlx::query("INSERT INTO mini_queue_states VALUES (1)")
            .execute(&writer)
            .await
            .unwrap();
        for events in [&mut first_events, &mut second_events] {
            assert!(
                tokio::time::timeout(Duration::from_millis(150), events.recv())
                    .await
                    .is_err(),
                "no-op and unrelated queue traffic must not rebuild inventory"
            );
        }
        // Location apparatus configuration and authorization dependencies are
        // first-class projection inputs, not just raw stock changes.
        for query in [
            "INSERT INTO mini_factory_location_apparatus_links VALUES (1)",
            "INSERT INTO mini_auth_accounts VALUES (1)",
            "TRUNCATE mini_raw_material_stock",
        ] {
            sqlx::query(query).execute(&writer).await.unwrap();
            for events in [&mut first_events, &mut second_events] {
                tokio::time::timeout(Duration::from_secs(5), events.recv())
                    .await
                    .unwrap()
                    .unwrap();
            }
        }
        // Simulate a lost LISTEN connection. Its next connection must emit an
        // invalidation even if the missed transaction no longer has a NOTIFY.
        sqlx::query("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1 AND application_name = $2 AND pid <> pg_backend_pid()")
            .bind(&name).bind(&first_name).execute(&writer).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), first_events.recv())
            .await
            .unwrap()
            .unwrap();
        first_pool.close().await;
        second_pool.close().await;
        tokio::time::timeout(Duration::from_secs(5), first_task)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), second_task)
            .await
            .unwrap()
            .unwrap();
        writer.close().await;
        sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
    }
}
