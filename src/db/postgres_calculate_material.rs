use async_trait::async_trait;
use sqlx::{PgConnection, PgPool, Row};

use crate::core::calculate_materials::{
    CalculateMaterial, CalculateMaterialError, CalculateMaterialStorePort, CalculateMaterialUpsert,
    merge_default_calculate_materials, normalize_material, prepare_material_upsert,
    reorder_calculate_materials,
};

#[derive(Clone)]
pub struct PostgresCalculateMaterialStore {
    pool: PgPool,
}

impl PostgresCalculateMaterialStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn list_on(
        conn: &mut PgConnection,
    ) -> Result<Vec<CalculateMaterial>, CalculateMaterialError> {
        let rows = sqlx::query(
            "SELECT id, payload_json
             FROM mini_calculate_materials
             ORDER BY lower_name",
        )
        .fetch_all(conn)
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

    async fn save_on(
        conn: &mut PgConnection,
        material: &CalculateMaterial,
    ) -> Result<(), CalculateMaterialError> {
        let payload =
            serde_json::to_value(material).map_err(|_| CalculateMaterialError::StoreFailed)?;
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
        .execute(conn)
        .await
        .map_err(|_| CalculateMaterialError::StoreFailed)?;
        Ok(())
    }
}

#[async_trait]
impl CalculateMaterialStorePort for PostgresCalculateMaterialStore {
    async fn list(&self) -> Result<Vec<CalculateMaterial>, CalculateMaterialError> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        Self::list_on(&mut conn).await
    }

    async fn upsert(
        &self,
        input: CalculateMaterialUpsert,
    ) -> Result<CalculateMaterial, CalculateMaterialError> {
        let mut material = normalize_material(input)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        // Serialize catalog edits and sequence writes, including builtins that
        // do not have a database row until their first edit or reorder.
        sqlx::query("LOCK TABLE mini_calculate_materials IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        let current = Self::list_on(&mut tx).await?;
        if let Some(pinned) = prepare_material_upsert(&current, &mut material)? {
            for item in &pinned {
                Self::save_on(&mut tx, item).await?;
            }
        } else {
            Self::save_on(&mut tx, &material).await?;
        }
        tx.commit()
            .await
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        Ok(material)
    }

    async fn reorder(
        &self,
        material_ids: Vec<String>,
    ) -> Result<Vec<CalculateMaterial>, CalculateMaterialError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        sqlx::query("LOCK TABLE mini_calculate_materials IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        let materials = reorder_calculate_materials(Self::list_on(&mut tx).await?, &material_ids)?;
        for material in &materials {
            Self::save_on(&mut tx, material).await?;
        }
        tx.commit()
            .await
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        Ok(materials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgConnectOptions;

    #[tokio::test]
    async fn calculate_material_postgres_sequence_persists_and_rolls_back() {
        let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .expect("isolated test database connection");
        let admin = PgPool::connect(&url).await.unwrap();
        let schema = format!("test_material_sequence_{:016x}", rand::random::<u64>());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let options = url
            .parse::<PgConnectOptions>()
            .unwrap()
            .options([("search_path", schema.as_str())]);
        let pool = PgPool::connect_with(options).await.unwrap();
        sqlx::raw_sql(
            "CREATE TABLE mini_calculate_materials (
            id TEXT PRIMARY KEY, lower_name TEXT NOT NULL UNIQUE,
            payload_json JSONB NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )",
        )
        .execute(&pool)
        .await
        .unwrap();
        let store = PostgresCalculateMaterialStore::new(pool.clone());
        store
            .upsert(CalculateMaterialUpsert {
                id: "builtin-bopp".into(),
                name: "BOPP".into(),
                active: false,
                density_g_cm3: 0.905,
                ..Default::default()
            })
            .await
            .unwrap();
        let ids: Vec<_> = store
            .list()
            .await
            .unwrap()
            .iter()
            .filter(|material| material.active)
            .rev()
            .map(|material| material.id.clone())
            .collect();
        let saved = store.reorder(ids.clone()).await.unwrap();
        assert_eq!(saved.last().unwrap().id, "builtin-bopp");
        let reopened = PostgresCalculateMaterialStore::new(pool.clone());
        assert_eq!(reopened.list().await.unwrap(), saved);
        let edited = reopened
            .upsert(CalculateMaterialUpsert {
                id: ids[0].clone(),
                name: "ZZZ renamed material".into(),
                density_g_cm3: 0.94,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(edited.sort_order, Some(0));
        assert_eq!(reopened.list().await.unwrap()[0].id, ids[0]);
        let before = reopened.list().await.unwrap();
        assert!(reopened.reorder(vec!["builtin-bopp".into()]).await.is_err());
        assert_eq!(reopened.list().await.unwrap(), before);
        sqlx::raw_sql(&format!(
            "CREATE FUNCTION fail_sequence() RETURNS TRIGGER AS $$
            BEGIN IF OLD.id = '{}' THEN RAISE EXCEPTION 'write failed'; END IF; RETURN NEW; END;
            $$ LANGUAGE plpgsql;
            CREATE TRIGGER fail_sequence BEFORE UPDATE ON mini_calculate_materials
                FOR EACH ROW EXECUTE FUNCTION fail_sequence();",
            ids[ids.len() - 2]
        ))
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            reopened
                .reorder(ids.into_iter().rev().collect())
                .await
                .is_err()
        );
        assert_eq!(reopened.list().await.unwrap(), before);
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
    }
}
