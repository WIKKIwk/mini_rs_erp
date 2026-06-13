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
}

fn group_id(name: &str) -> String {
    format!("apparatus_group:{}", name.trim().to_lowercase())
}
