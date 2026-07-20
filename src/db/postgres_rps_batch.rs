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

    async fn complete(&self, batch: RpsBatchSession) -> Result<(), RpsBatchStoreError> {
        let payload = serde_json::to_value(&batch).map_err(|_| RpsBatchStoreError::StoreFailed)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| RpsBatchStoreError::StoreFailed)?;
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
        .bind(payload.clone())
        .execute(&mut *tx)
        .await
        .map_err(|_| RpsBatchStoreError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_rps_batch_history
                (batch_id, owner_key, owner_role, owner_ref, item_code, warehouse,
                 payload_json, completed_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, now())
             ON CONFLICT (owner_key, batch_id) DO UPDATE SET
                owner_role = excluded.owner_role,
                owner_ref = excluded.owner_ref,
                item_code = excluded.item_code,
                warehouse = excluded.warehouse,
                payload_json = excluded.payload_json",
        )
        .bind(batch.id.trim())
        .bind(batch.owner_key.trim())
        .bind(batch.owner_role.trim())
        .bind(batch.owner_ref.trim())
        .bind(batch.item_code.trim())
        .bind(batch.warehouse.trim())
        .bind(payload)
        .execute(&mut *tx)
        .await
        .map_err(|_| RpsBatchStoreError::StoreFailed)?;
        tx.commit()
            .await
            .map_err(|_| RpsBatchStoreError::StoreFailed)
    }

    async fn list_completed(
        &self,
        owner_key: &str,
        limit: usize,
    ) -> Result<Vec<RpsBatchSession>, RpsBatchStoreError> {
        let payloads = sqlx::query_scalar::<_, serde_json::Value>(
            "WITH completed_batches AS (
                 SELECT
                     jsonb_set(
                         history.payload_json,
                         '{prints}',
                         COALESCE((
                             SELECT jsonb_agg(
                                 printed.entry || CASE
                                     WHEN receipt.barcode IS NULL THEN '{}'::jsonb
                                     ELSE jsonb_build_object(
                                         'draft_name', receipt.name,
                                         'status', receipt.status
                                     )
                                 END
                                 ORDER BY printed.ordinality
                             )
                             FROM jsonb_array_elements(
                                 COALESCE(history.payload_json->'prints', '[]'::jsonb)
                             ) WITH ORDINALITY AS printed(entry, ordinality)
                             LEFT JOIN mini_gscale_receipts AS receipt
                               ON receipt.barcode = printed.entry->>'epc'
                         ), '[]'::jsonb),
                         true
                     ) AS payload,
                     history.completed_at AS sort_at,
                     history.batch_id AS sort_id
                 FROM mini_rps_batch_history AS history
                 WHERE history.owner_key = $1
             ), legacy_receipts AS (
                 SELECT
                     jsonb_build_object(
                         'id', 'legacy_receipt_' || receipt.barcode,
                         'active', false,
                         'owner_key', $1,
                         'owner_role', receipt.payload_json->>'actor_role',
                         'owner_ref', receipt.payload_json->>'actor_ref',
                         'driver_url', '',
                         'item_code', receipt.item_code,
                         'item_name', COALESCE(receipt.payload_json->>'item_name', receipt.item_code),
                         'warehouse', receipt.warehouse,
                         'printer', '',
                         'print_mode', '',
                         'quantity_source', 'receipt',
                         'manual_qty_kg', 0,
                         'tare_enabled', false,
                         'tare_kg', 0,
                         'last_error', '',
                         'last_error_at', '',
                         'prints', jsonb_build_array(jsonb_build_object(
                             'epc', receipt.barcode,
                             'draft_name', receipt.name,
                             'status', receipt.status,
                             'qty', receipt.qty,
                             'net_qty', receipt.qty,
                             'gross_qty', receipt.qty,
                             'unit', receipt.uom,
                             'printer', '',
                             'print_mode', '',
                             'print_count', 1,
                             'printed_at', receipt.created_at
                         )),
                         'created_at', receipt.created_at,
                         'updated_at', receipt.updated_at
                     ) AS payload,
                     receipt.updated_at AS sort_at,
                     receipt.barcode AS sort_id
                 FROM mini_gscale_receipts AS receipt
                 WHERE concat(
                           receipt.payload_json->>'actor_role',
                           ':',
                           receipt.payload_json->>'actor_ref'
                       ) = $1
                   AND NOT EXISTS (
                       SELECT 1
                       FROM mini_rps_batch_history AS history
                       CROSS JOIN LATERAL jsonb_array_elements(
                           COALESCE(history.payload_json->'prints', '[]'::jsonb)
                       ) AS printed(entry)
                       WHERE history.owner_key = $1
                         AND printed.entry->>'epc' = receipt.barcode
                   )
             )
             SELECT payload
             FROM (
                 SELECT * FROM completed_batches
                 UNION ALL
                 SELECT * FROM legacy_receipts
             ) AS history
             ORDER BY sort_at DESC, sort_id DESC
             LIMIT $2",
        )
        .bind(owner_key.trim())
        .bind(limit.min(100) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| RpsBatchStoreError::StoreFailed)?;
        payloads
            .into_iter()
            .map(|value| {
                serde_json::from_value::<RpsBatchSession>(value)
                    .map_err(|_| RpsBatchStoreError::StoreFailed)
            })
            .collect()
    }
}
