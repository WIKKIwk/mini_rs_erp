use async_trait::async_trait;
use sqlx::{PgPool, Row};

use crate::core::apparatus_groups::{
    ApparatusCatalogEntry, ApparatusGroup, ApparatusGroupError, ApparatusGroupStorePort,
    ApparatusMasterData, ApparatusSource, apparatus_master_data_for_name, custom_apparatus_id,
};

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

    async fn apparatus_catalog(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ApparatusCatalogEntry>, ApparatusGroupError> {
        let needle = query.trim().to_lowercase();
        let pattern = format!("%{needle}%");
        let rows = sqlx::query(
            "SELECT id, name, payload_json
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
        .map_err(|_| ApparatusGroupError::StoreFailed)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let name: String = row.get("name");
                let payload: serde_json::Value = row.get("payload_json");
                let master = serde_json::from_value::<ApparatusMasterData>(payload)
                    .unwrap_or_else(|_| apparatus_master_data_for_name(&name));
                ApparatusCatalogEntry {
                    id: row.get("id"),
                    name,
                    source: ApparatusSource::Custom,
                    sort_order: 0,
                    master,
                }
            })
            .collect())
    }

    async fn put_apparatus(&self, name: &str) -> Result<String, ApparatusGroupError> {
        self.save_apparatus(name, &apparatus_master_data_for_name(name))
            .await
    }

    async fn put_apparatus_with_master_data(
        &self,
        name: &str,
        master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
        self.save_apparatus(name, master).await
    }
}

impl PostgresApparatusGroupStore {
    async fn save_apparatus(
        &self,
        name: &str,
        master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        let mut payload =
            serde_json::to_value(master).map_err(|_| ApparatusGroupError::StoreFailed)?;
        payload["warehouse"] = serde_json::Value::String(name.to_string());
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
            .bind(payload.clone())
            .fetch_one(&self.pool)
            .await
            .map_err(|_| ApparatusGroupError::StoreFailed);
        }

        sqlx::query_scalar::<_, String>(
            "INSERT INTO mini_apparatus (id, name, base_name, kind, payload_json)
             VALUES ($1, $2, $2, 'custom', $3)
             RETURNING name",
        )
        .bind(apparatus_id(name))
        .bind(name)
        .bind(payload)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| ApparatusGroupError::StoreFailed)
    }
}

fn group_id(name: &str) -> String {
    format!("apparatus_group:{}", name.trim().to_lowercase())
}

fn apparatus_id(name: &str) -> String {
    custom_apparatus_id(name)
}
