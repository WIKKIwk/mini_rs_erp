use async_trait::async_trait;
use sqlx::PgPool;

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
             WHERE ($1 = '' OR lower(name) LIKE $2 OR lower(phone) LIKE $2 OR lower(level) LIKE $2)
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
             WHERE id = ANY($1)
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
        let (id, name, phone, level) = sqlx::query_as::<_, (String, String, String, String)>(
            "INSERT INTO mini_workers (id, name, phone, level, payload_json)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT ((lower(name))) DO UPDATE SET
               name = excluded.name,
               phone = excluded.phone,
               level = excluded.level,
               payload_json = excluded.payload_json,
               updated_at = now()
             RETURNING id, name, COALESCE(phone, ''), level",
        )
        .bind(worker.id)
        .bind(worker.name)
        .bind(worker.phone)
        .bind(worker.level)
        .bind(payload)
        .fetch_one(&self.pool)
        .await
        .map_err(map_worker_write_error)?;

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
             WHERE id = $1
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
        let row = sqlx::query_as::<_, (String, String, String, String)>(
            "UPDATE mini_workers
             SET phone = $2,
                 payload_json = jsonb_set(payload_json, '{phone}', to_jsonb($2::text), true),
                 updated_at = now()
             WHERE id = $1
             RETURNING id, name, COALESCE(phone, ''), level",
        )
        .bind(id.trim())
        .bind(phone.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_worker_write_error)?;

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

    async fn delete_worker(&self, id: &str) -> Result<(), WorkerError> {
        let result = sqlx::query("DELETE FROM mini_workers WHERE id = $1")
            .bind(id.trim())
            .execute(&self.pool)
            .await
            .map_err(|_| WorkerError::StoreFailed)?;
        if result.rows_affected() == 0 {
            return Err(WorkerError::NotFound);
        }
        Ok(())
    }
}

fn map_worker_write_error(error: sqlx::Error) -> WorkerError {
    if error
        .as_database_error()
        .and_then(|error| error.constraint())
        == Some("idx_mini_workers_phone_key_unique")
    {
        WorkerError::DuplicatePhone
    } else {
        WorkerError::StoreFailed
    }
}
