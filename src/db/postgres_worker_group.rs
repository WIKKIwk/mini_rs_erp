use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::worker_groups::{WorkerGroupError, WorkerGroupRecord, WorkerGroupStorePort};

#[derive(Clone)]
pub struct PostgresWorkerGroupStore {
    pool: PgPool,
}

impl PostgresWorkerGroupStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WorkerGroupStorePort for PostgresWorkerGroupStore {
    async fn worker_groups(
        &self,
        apparatus: Option<&str>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        let apparatus = apparatus.unwrap_or("").trim().to_lowercase();
        let rows = sqlx::query_as::<_, (String, String, String, serde_json::Value)>(
            "SELECT apparatus, group_code, shift, worker_ids
             FROM mini_worker_groups
             WHERE ($1 = '' OR lower(apparatus) = $1)
             ORDER BY lower(apparatus) ASC, group_code ASC",
        )
        .bind(apparatus)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| WorkerGroupError::StoreFailed)?;

        rows.into_iter()
            .map(|(apparatus, group_code, shift, worker_ids)| {
                let worker_ids = serde_json::from_value::<Vec<String>>(worker_ids)
                    .map_err(|_| WorkerGroupError::StoreFailed)?;
                Ok(WorkerGroupRecord {
                    apparatus,
                    group_code,
                    shift,
                    worker_ids,
                })
            })
            .collect()
    }

    async fn put_apparatus_worker_groups(
        &self,
        apparatus: &str,
        groups: Vec<WorkerGroupRecord>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| WorkerGroupError::StoreFailed)?;
        sqlx::query("DELETE FROM mini_worker_groups WHERE lower(apparatus) = lower($1)")
            .bind(apparatus.trim())
            .execute(&mut *tx)
            .await
            .map_err(|_| WorkerGroupError::StoreFailed)?;

        for group in &groups {
            let worker_ids = serde_json::to_value(&group.worker_ids)
                .map_err(|_| WorkerGroupError::StoreFailed)?;
            let payload = serde_json::to_value(group).map_err(|_| WorkerGroupError::StoreFailed)?;
            sqlx::query(
                "INSERT INTO mini_worker_groups
                    (apparatus, group_code, shift, worker_ids, payload_json)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(&group.apparatus)
            .bind(&group.group_code)
            .bind(&group.shift)
            .bind(worker_ids)
            .bind(payload)
            .execute(&mut *tx)
            .await
            .map_err(|_| WorkerGroupError::StoreFailed)?;
        }

        tx.commit()
            .await
            .map_err(|_| WorkerGroupError::StoreFailed)?;
        Ok(groups)
    }
}
