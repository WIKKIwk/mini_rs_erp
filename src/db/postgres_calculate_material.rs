use async_trait::async_trait;
use sqlx::{PgPool, Row};

use crate::core::calculate_materials::{
    CalculateMaterial, CalculateMaterialError, CalculateMaterialStorePort,
    CalculateMaterialUpsert, ensure_unique_name, merge_default_calculate_materials,
    normalize_material,
};

#[derive(Clone)]
pub struct PostgresCalculateMaterialStore {
    pool: PgPool,
}

impl PostgresCalculateMaterialStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CalculateMaterialStorePort for PostgresCalculateMaterialStore {
    async fn list(&self) -> Result<Vec<CalculateMaterial>, CalculateMaterialError> {
        let rows = sqlx::query(
            "SELECT id, payload_json
             FROM mini_calculate_materials
             ORDER BY lower_name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| CalculateMaterialError::StoreFailed)?;

        let overrides = rows
            .into_iter()
            .map(|row| {
                let payload = row
                    .try_get::<serde_json::Value, _>("payload_json")
                    .map_err(|_| CalculateMaterialError::StoreFailed)?;
                serde_json::from_value::<CalculateMaterial>(payload)
                    .map_err(|_| CalculateMaterialError::StoreFailed)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(merge_default_calculate_materials(overrides))
    }

    async fn upsert(
        &self,
        input: CalculateMaterialUpsert,
    ) -> Result<CalculateMaterial, CalculateMaterialError> {
        let material = normalize_material(input)?;
        let current = self.list().await?;
        ensure_unique_name(&current, &material)?;
        let payload = serde_json::to_value(&material)
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_calculate_materials
                (id, lower_name, payload_json, updated_at)
             VALUES ($1, lower($2), $3, now())
             ON CONFLICT (id) DO UPDATE SET
                lower_name = excluded.lower_name,
                payload_json = excluded.payload_json,
                updated_at = excluded.updated_at",
        )
        .bind(&material.id)
        .bind(&material.name)
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(|_| CalculateMaterialError::StoreFailed)?;
        Ok(material)
    }
}
