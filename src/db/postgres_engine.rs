use sqlx::PgPool;

use crate::engine::{EngineCommandContext, EngineEventDraft};

#[derive(Clone)]
#[allow(dead_code)]
pub struct PostgresEngineStore {
    pool: PgPool,
}

impl PostgresEngineStore {
    #[allow(dead_code)]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    #[allow(dead_code)]
    pub async fn reserve_idempotency_key(
        &self,
        context: &EngineCommandContext,
        domain: &str,
        action: &str,
        entity_id: &str,
    ) -> Result<IdempotencyReservation, sqlx::Error> {
        let result = sqlx::query(
            "INSERT INTO mini_idempotency_keys (key, domain, action, entity_id)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (key) DO NOTHING",
        )
        .bind(context.idempotency_key.trim())
        .bind(domain.trim())
        .bind(action.trim())
        .bind(entity_id.trim())
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            Ok(IdempotencyReservation::AlreadyReserved)
        } else {
            Ok(IdempotencyReservation::Reserved)
        }
    }

    #[allow(dead_code)]
    pub async fn record_event(&self, event: &EngineEventDraft) -> Result<String, sqlx::Error> {
        let event_id = new_event_id();
        sqlx::query(
            "INSERT INTO mini_engine_events
                (event_id, domain, action, entity_id, actor_key, idempotency_key, payload_json)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&event_id)
        .bind(event.domain.trim())
        .bind(event.action.trim())
        .bind(event.entity_id.trim())
        .bind(event.actor_key.trim())
        .bind(event.idempotency_key.trim())
        .bind(&event.payload_json)
        .execute(&self.pool)
        .await?;
        Ok(event_id)
    }

    #[allow(dead_code)]
    pub async fn complete_idempotency_key(
        &self,
        context: &EngineCommandContext,
        response_json: serde_json::Value,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE mini_idempotency_keys
             SET response_json = $2, completed_at = now()
             WHERE key = $1",
        )
        .bind(context.idempotency_key.trim())
        .bind(response_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum IdempotencyReservation {
    Reserved,
    AlreadyReserved,
}

fn new_event_id() -> String {
    format!("evt_{}", new_hex_id())
}

fn new_hex_id() -> String {
    let bytes: [u8; 16] = rand::random();
    data_encoding::HEXLOWER.encode(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::postgres::apply_foundation_migration;
    use crate::engine::{EngineCommandContext, EngineEventDraft};

    #[tokio::test]
    #[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_engine"]
    async fn postgres_engine_store_reserves_idempotency_and_records_events() {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let db_name = "mini_rs_erp_test_engine";
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
            .expect("apply migration");
        let store = PostgresEngineStore::new(pool.clone());

        let context = EngineCommandContext::new("admin:admin", "move-zakaz-1001").expect("context");
        let first = store
            .reserve_idempotency_key(&context, "production_maps", "batch_move", "zakaz-1001")
            .await
            .expect("reserve first");
        assert_eq!(first, IdempotencyReservation::Reserved);

        let duplicate = store
            .reserve_idempotency_key(&context, "production_maps", "batch_move", "zakaz-1001")
            .await
            .expect("reserve duplicate");
        assert_eq!(duplicate, IdempotencyReservation::AlreadyReserved);

        let event = EngineEventDraft::new(
            &context,
            "production_maps",
            "batch_move",
            "zakaz-1001",
            serde_json::json!({"from":"7 ta rangli pechat","to":"8 ta rangli pechat"}),
        )
        .expect("event");
        let event_id = store.record_event(&event).await.expect("record event");
        assert!(event_id.starts_with("evt_"));

        store
            .complete_idempotency_key(&context, serde_json::json!({"ok":true}))
            .await
            .expect("complete key");

        let completed: Option<serde_json::Value> =
            sqlx::query_scalar("SELECT response_json FROM mini_idempotency_keys WHERE key = $1")
                .bind(&context.idempotency_key)
                .fetch_one(&pool)
                .await
                .expect("read response");
        assert_eq!(completed, Some(serde_json::json!({"ok":true})));

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
}
