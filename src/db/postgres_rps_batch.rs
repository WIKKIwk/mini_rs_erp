use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::rps_batch::models::RpsBatchSession;
use crate::core::rps_batch::ports::{RpsBatchStoreError, RpsBatchStorePort};

#[derive(Clone)]
pub struct PostgresRpsBatchStore {
    pool: PgPool,
}

impl PostgresRpsBatchStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RpsBatchStorePort for PostgresRpsBatchStore {
    async fn get(&self, owner_key: &str) -> Result<Option<RpsBatchSession>, RpsBatchStoreError> {
        let payload = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json
             FROM mini_rps_batches
             WHERE owner_key = $1",
        )
        .bind(owner_key.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| RpsBatchStoreError::StoreFailed)?;
        payload
            .map(|value| {
                serde_json::from_value::<RpsBatchSession>(value)
                    .map_err(|_| RpsBatchStoreError::StoreFailed)
            })
            .transpose()
    }

    async fn put(&self, batch: RpsBatchSession) -> Result<(), RpsBatchStoreError> {
        let payload = serde_json::to_value(&batch).map_err(|_| RpsBatchStoreError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_rps_batches
                (owner_key, batch_id, active, owner_role, owner_ref, item_code, warehouse,
                 payload_json, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
             ON CONFLICT (owner_key) DO UPDATE SET
                batch_id = excluded.batch_id,
                active = excluded.active,
                owner_role = excluded.owner_role,
                owner_ref = excluded.owner_ref,
                item_code = excluded.item_code,
                warehouse = excluded.warehouse,
                payload_json = excluded.payload_json,
                updated_at = excluded.updated_at",
        )
        .bind(batch.owner_key.trim())
        .bind(batch.id.trim())
        .bind(batch.active)
        .bind(batch.owner_role.trim())
        .bind(batch.owner_ref.trim())
        .bind(batch.item_code.trim())
        .bind(batch.warehouse.trim())
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(|_| RpsBatchStoreError::StoreFailed)?;
        Ok(())
    }
}
