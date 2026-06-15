use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::apparatus_groups::{ApparatusGroup, ApparatusGroupError, ApparatusGroupStorePort};

#[derive(Clone)]
pub struct PostgresApparatusGroupStore {
    pool: PgPool,
}

impl PostgresApparatusGroupStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ApparatusGroupStorePort for PostgresApparatusGroupStore {
    async fn groups(&self) -> Result<Vec<ApparatusGroup>, ApparatusGroupError> {
        let rows = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json
             FROM mini_apparatus_groups
             ORDER BY lower(name) ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)?;

        rows.into_iter()
            .map(|payload| {
                serde_json::from_value::<ApparatusGroup>(payload)
                    .map_err(|_| ApparatusGroupError::StoreFailed)
            })
            .collect()
    }

    async fn put_group(&self, group: ApparatusGroup) -> Result<(), ApparatusGroupError> {
        let name = group.name.trim();
        let group_id = group_id(name);
        let payload = serde_json::to_value(&group).map_err(|_| ApparatusGroupError::StoreFailed)?;

        sqlx::query(
            "INSERT INTO mini_apparatus_groups (id, name, payload_json)
             VALUES ($1, $2, $3)
             ON CONFLICT ((lower(name))) DO UPDATE SET
               name = excluded.name,
               payload_json = excluded.payload_json,
               updated_at = now()",
        )
        .bind(group_id)
        .bind(name)
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)?;

        Ok(())
    }

    async fn apparatus(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>, ApparatusGroupError> {
        let needle = query.trim().to_lowercase();
        let pattern = format!("%{needle}%");
        sqlx::query_scalar::<_, String>(
            "SELECT name
             FROM mini_apparatus
             WHERE ($1 = '' OR lower(name) LIKE $2)
             ORDER BY lower(name) ASC
             LIMIT $3",
        )
        .bind(needle)
        .bind(pattern)
        .bind(limit.max(1) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)
    }

    async fn put_apparatus(&self, name: &str) -> Result<String, ApparatusGroupError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        let existing_id = sqlx::query_scalar::<_, String>(
            "SELECT id
             FROM mini_apparatus
             WHERE lower(name) = lower($1)
             LIMIT 1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)?;

        if let Some(id) = existing_id {
            return sqlx::query_scalar::<_, String>(
                "UPDATE mini_apparatus
                 SET name = $2, payload_json = $3, updated_at = now()
                 WHERE id = $1
                 RETURNING name",
            )
            .bind(id)
            .bind(name)
            .bind(serde_json::json!({"warehouse": name}))
            .fetch_one(&self.pool)
            .await
            .map_err(|_| ApparatusGroupError::StoreFailed);
        }

        sqlx::query_scalar::<_, String>(
            "INSERT INTO mini_apparatus (id, name, payload_json)
             VALUES ($1, $2, $3)
             RETURNING name",
        )
        .bind(apparatus_id(name))
        .bind(name)
        .bind(serde_json::json!({"warehouse": name}))
        .fetch_one(&self.pool)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)
    }
}

fn group_id(name: &str) -> String {
    format!("apparatus_group:{}", name.trim().to_lowercase())
}

fn apparatus_id(name: &str) -> String {
    format!("apparatus:{}", name.trim().to_lowercase())
}
