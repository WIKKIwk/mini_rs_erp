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
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT id, name, level
             FROM mini_workers
             WHERE ($1 = '' OR lower(name) LIKE $2 OR lower(level) LIKE $2)
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
            .map(|(id, name, level)| Worker { id, name, level })
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
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT id, name, level
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
            .map(|(id, name, level)| Worker { id, name, level })
            .collect())
    }

    async fn upsert_worker(&self, worker: Worker) -> Result<Worker, WorkerError> {
        let payload = serde_json::to_value(&worker).map_err(|_| WorkerError::StoreFailed)?;
        let (id, name, level) = sqlx::query_as::<_, (String, String, String)>(
            "INSERT INTO mini_workers (id, name, level, payload_json)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT ((lower(name))) DO UPDATE SET
               name = excluded.name,
               level = excluded.level,
               payload_json = excluded.payload_json,
               updated_at = now()
             RETURNING id, name, level",
        )
        .bind(worker.id)
        .bind(worker.name)
        .bind(worker.level)
        .bind(payload)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| WorkerError::StoreFailed)?;

        Ok(Worker { id, name, level })
    }

    async fn update_worker_level(&self, id: &str, level: &str) -> Result<Worker, WorkerError> {
        let row = sqlx::query_as::<_, (String, String, String)>(
            "UPDATE mini_workers
             SET level = $2,
                 payload_json = jsonb_set(payload_json, '{level}', to_jsonb($2::text), true),
                 updated_at = now()
             WHERE id = $1
             RETURNING id, name, level",
        )
        .bind(id.trim())
        .bind(level.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| WorkerError::StoreFailed)?;

        let Some((id, name, level)) = row else {
            return Err(WorkerError::NotFound);
        };
        Ok(Worker { id, name, level })
    }
}
