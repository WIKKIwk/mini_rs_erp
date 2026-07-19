use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::workers::{Worker, WorkerError, WorkerStorePort};

#[derive(Clone)]
pub struct PostgresWorkerStore {
    pool: PgPool,
}

impl PostgresWorkerStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WorkerStorePort for PostgresWorkerStore {
    async fn workers(&self, query: &str, limit: usize) -> Result<Vec<Worker>, WorkerError> {
        let needle = query.trim().to_lowercase();
        let pattern = format!("%{needle}%");
        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT id, name, COALESCE(phone, ''), level
             FROM mini_workers
             WHERE active
               AND ($1 = '' OR lower(name) LIKE $2 OR lower(phone) LIKE $2 OR lower(level) LIKE $2)
             ORDER BY lower(name) ASC
             LIMIT $3",
        )
        .bind(needle)
        .bind(pattern)
        .bind(limit.max(1) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| WorkerError::StoreFailed)?;

        Ok(rows
            .into_iter()
            .map(|(id, name, phone, level)| Worker {
                id,
                name,
                phone,
                level,
            })
            .collect())
    }

    async fn workers_by_ids(&self, ids: &[String]) -> Result<Vec<Worker>, WorkerError> {
        let ids = ids
            .iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT id, name, COALESCE(phone, ''), level
             FROM mini_workers
             WHERE active AND id = ANY($1)
             ORDER BY array_position($1, id)",
        )
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| WorkerError::StoreFailed)?;

        Ok(rows
            .into_iter()
            .map(|(id, name, phone, level)| Worker {
                id,
                name,
                phone,
                level,
            })
            .collect())
    }

    async fn upsert_worker(&self, worker: Worker) -> Result<Worker, WorkerError> {
        let payload = serde_json::to_value(&worker).map_err(|_| WorkerError::StoreFailed)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| WorkerError::StoreFailed)?;
        let (id, name, phone, level) = sqlx::query_as::<_, (String, String, String, String)>(
            "INSERT INTO mini_workers (id, name, phone, level, payload_json, active, deactivated_at)
             VALUES ($1, $2, $3, $4, $5, TRUE, NULL)
             ON CONFLICT (id) DO UPDATE SET
               name = excluded.name,
               phone = excluded.phone,
               level = excluded.level,
               payload_json = excluded.payload_json,
               active = TRUE,
               deactivated_at = NULL,
               updated_at = now()
             RETURNING id, name, COALESCE(phone, ''), level",
        )
        .bind(worker.id)
        .bind(worker.name)
        .bind(worker.phone)
        .bind(worker.level)
        .bind(payload)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_worker_write_error)?;
        sync_phone_alias(&mut tx, &id, &phone)
            .await
            .map_err(map_worker_write_error)?;
        tx.commit().await.map_err(|_| WorkerError::StoreFailed)?;

        Ok(Worker {
            id,
            name,
            phone,
            level,
        })
    }

    async fn update_worker_level(&self, id: &str, level: &str) -> Result<Worker, WorkerError> {
        let row = sqlx::query_as::<_, (String, String, String, String)>(
            "UPDATE mini_workers
             SET level = $2,
                 payload_json = jsonb_set(payload_json, '{level}', to_jsonb($2::text), true),
                 updated_at = now()
             WHERE id = $1 AND active
             RETURNING id, name, COALESCE(phone, ''), level",
        )
        .bind(id.trim())
        .bind(level.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| WorkerError::StoreFailed)?;

        let Some((id, name, phone, level)) = row else {
            return Err(WorkerError::NotFound);
        };
        Ok(Worker {
            id,
            name,
            phone,
            level,
        })
    }

    async fn update_worker_phone(&self, id: &str, phone: &str) -> Result<Worker, WorkerError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| WorkerError::StoreFailed)?;
        let row = sqlx::query_as::<_, (String, String, String, String)>(
            "UPDATE mini_workers
             SET phone = $2,
                 payload_json = jsonb_set(payload_json, '{phone}', to_jsonb($2::text), true),
                 updated_at = now()
             WHERE id = $1 AND active
             RETURNING id, name, COALESCE(phone, ''), level",
        )
        .bind(id.trim())
        .bind(phone.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_worker_write_error)?;

        let Some((id, name, phone, level)) = row else {
            return Err(WorkerError::NotFound);
        };
        sync_phone_alias(&mut tx, &id, &phone)
            .await
            .map_err(map_worker_write_error)?;
        tx.commit().await.map_err(|_| WorkerError::StoreFailed)?;
        Ok(Worker {
            id,
            name,
            phone,
            level,
        })
    }

    async fn deactivate_worker(&self, id: &str) -> Result<(), WorkerError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| WorkerError::StoreFailed)?;
        let result = sqlx::query(
            "UPDATE mini_workers
             SET active = FALSE,
                 deactivated_at = now(),
                 updated_at = now()
             WHERE id = $1 AND active",
        )
        .bind(id.trim())
        .execute(&mut *tx)
        .await
        .map_err(|_| WorkerError::StoreFailed)?;
        if result.rows_affected() == 0 {
            return Err(WorkerError::NotFound);
        }
        sqlx::query(
            "UPDATE mini_worker_identity_aliases
             SET valid_to = now()
             WHERE worker_id = $1 AND valid_to IS NULL",
        )
        .bind(id.trim())
        .execute(&mut *tx)
        .await
        .map_err(|_| WorkerError::StoreFailed)?;
        tx.commit().await.map_err(|_| WorkerError::StoreFailed)?;
        Ok(())
    }
}

async fn sync_phone_alias(
    tx: &mut Transaction<'_, Postgres>,
    worker_id: &str,
    phone: &str,
) -> Result<(), sqlx::Error> {
    let phone_key = normalized_phone_key(phone);
    sqlx::query(
        "UPDATE mini_worker_identity_aliases
         SET valid_to = now()
         WHERE worker_id = $1
           AND alias_type = 'phone'
           AND valid_to IS NULL
           AND alias_key <> $2",
    )
    .bind(worker_id.trim())
    .bind(&phone_key)
    .execute(&mut **tx)
    .await?;

    if phone_key.is_empty() {
        return Ok(());
    }

    sqlx::query(
        "INSERT INTO mini_worker_identity_aliases (
             worker_id, alias_type, alias_key, valid_from
         )
         SELECT $1, 'phone', $2, now()
         WHERE NOT EXISTS (
             SELECT 1
             FROM mini_worker_identity_aliases
             WHERE worker_id = $1
               AND alias_type = 'phone'
               AND alias_key = $2
               AND valid_to IS NULL
         )",
    )
    .bind(worker_id.trim())
    .bind(phone_key)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn normalized_phone_key(phone: &str) -> String {
    phone.chars().filter(char::is_ascii_digit).collect()
}

fn map_worker_write_error(error: sqlx::Error) -> WorkerError {
    if error
        .as_database_error()
        .and_then(|error| error.constraint())
        .is_some_and(|constraint| {
            matches!(
                constraint,
                "idx_mini_workers_phone_key_unique"
                    | "idx_mini_workers_active_phone_key_unique"
                    | "idx_mini_worker_identity_alias_open_unique"
            )
        })
    {
        WorkerError::DuplicatePhone
    } else {
        WorkerError::StoreFailed
    }
}
