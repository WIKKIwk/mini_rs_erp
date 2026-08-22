use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use crate::core::apparatus_standard::{LifecycleState, RuntimeApparatusProjection};
use crate::core::factory_locations::{
    FactoryLocation, FactoryLocationApparatus, FactoryLocationError, FactoryLocationStorePort,
};

#[derive(Clone)]
pub struct PostgresFactoryLocationStore {
    pool: PgPool,
}

impl PostgresFactoryLocationStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FactoryLocationStorePort for PostgresFactoryLocationStore {
    async fn list(&self) -> Result<Vec<FactoryLocation>, FactoryLocationError> {
        load_locations(&self.pool, None).await
    }

    async fn create(
        &self,
        id: &str,
        name: &str,
        apparatus: &[FactoryLocationApparatus],
    ) -> Result<FactoryLocation, FactoryLocationError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| FactoryLocationError::StoreFailed)?;
        let row = sqlx::query(
            "INSERT INTO mini_factory_locations (id, name)
             VALUES ($1, $2)
             RETURNING active,
               EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
               EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix",
        )
        .bind(id)
        .bind(name)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_sql_error)?;
        insert_links(&mut tx, id, apparatus).await?;
        tx.commit()
            .await
            .map_err(|_| FactoryLocationError::StoreFailed)?;
        Ok(FactoryLocation {
            id: id.to_string(),
            name: name.to_string(),
            active: row.get("active"),
            apparatus: apparatus.to_vec(),
            created_at_unix: row.get("created_at_unix"),
            updated_at_unix: row.get("updated_at_unix"),
        })
    }

    async fn update(
        &self,
        id: &str,
        name: Option<&str>,
        active: Option<bool>,
    ) -> Result<FactoryLocation, FactoryLocationError> {
        let result = sqlx::query(
            "UPDATE mini_factory_locations
             SET name = COALESCE($2, name),
                 active = COALESCE($3, active),
                 updated_at = now()
             WHERE id = $1",
        )
        .bind(id)
        .bind(name)
        .bind(active)
        .execute(&self.pool)
        .await
        .map_err(map_sql_error)?;
        if result.rows_affected() == 0 {
            return Err(FactoryLocationError::NotFound);
        }
        load_one(&self.pool, id).await
    }

    async fn replace_apparatus(
        &self,
        id: &str,
        apparatus: &[FactoryLocationApparatus],
    ) -> Result<FactoryLocation, FactoryLocationError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| FactoryLocationError::StoreFailed)?;
        let row = sqlx::query(
            "UPDATE mini_factory_locations
             SET updated_at = now()
             WHERE id = $1
             RETURNING name, active,
               EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
               EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sql_error)?
        .ok_or(FactoryLocationError::NotFound)?;
        sqlx::query(
            "DELETE FROM mini_factory_location_apparatus_links
             WHERE location_id = $1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(map_sql_error)?;
        insert_links(&mut tx, id, apparatus).await?;
        tx.commit()
            .await
            .map_err(|_| FactoryLocationError::StoreFailed)?;
        Ok(FactoryLocation {
            id: id.to_string(),
            name: row.get("name"),
            active: row.get("active"),
            apparatus: apparatus.to_vec(),
            created_at_unix: row.get("created_at_unix"),
            updated_at_unix: row.get("updated_at_unix"),
        })
    }
}

async fn insert_links(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    location_id: &str,
    apparatus: &[FactoryLocationApparatus],
) -> Result<(), FactoryLocationError> {
    for item in apparatus {
        sqlx::query(
            "INSERT INTO mini_factory_location_apparatus_links (location_id, apparatus_id)
             VALUES ($1, $2)",
        )
        .bind(location_id)
        .bind(item.id.as_str())
        .execute(&mut **tx)
        .await
        .map_err(map_sql_error)?;
    }
    Ok(())
}

async fn load_one(pool: &PgPool, id: &str) -> Result<FactoryLocation, FactoryLocationError> {
    load_locations(pool, Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or(FactoryLocationError::NotFound)
}

async fn load_locations(
    pool: &PgPool,
    id: Option<&str>,
) -> Result<Vec<FactoryLocation>, FactoryLocationError> {
    let location_rows = sqlx::query(
        "SELECT id, name, active,
           EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
           EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix
         FROM mini_factory_locations
         WHERE ($1::TEXT IS NULL OR id = $1)
         ORDER BY lower(name) ASC, id ASC",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(map_sql_error)?;

    let link_rows = sqlx::query(
        "SELECT links.location_id, apparatus.payload_json AS runtime_payload
         FROM mini_factory_location_apparatus_links links
         JOIN mini_apparatus apparatus ON apparatus.id = links.apparatus_id
          AND apparatus.source_revision IS NOT NULL
         WHERE ($1::TEXT IS NULL OR links.location_id = $1)
         ORDER BY links.location_id,
                  links.apparatus_id ASC",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(map_sql_error)?;

    let mut apparatus_by_location = BTreeMap::<String, Vec<FactoryLocationApparatus>>::new();
    for row in link_rows {
        let runtime =
            serde_json::from_value::<RuntimeApparatusProjection>(row.get("runtime_payload"))
                .map_err(|_| FactoryLocationError::InvalidApparatus)?;
        apparatus_by_location
            .entry(row.get("location_id"))
            .or_default()
            .push(FactoryLocationApparatus {
                id: runtime.apparatus_id,
                name: runtime.display.display_name,
                source_revision: runtime.source_revision,
                equipment_class_id: runtime.equipment_class_id,
                physical_asset_id: runtime.physical_asset_id,
                active: runtime.lifecycle.state == LifecycleState::Active,
            });
    }

    Ok(location_rows
        .into_iter()
        .map(|row| {
            let id: String = row.get("id");
            FactoryLocation {
                apparatus: apparatus_by_location.remove(&id).unwrap_or_default(),
                id,
                name: row.get("name"),
                active: row.get("active"),
                created_at_unix: row.get("created_at_unix"),
                updated_at_unix: row.get("updated_at_unix"),
            }
        })
        .collect())
}

fn map_sql_error(error: sqlx::Error) -> FactoryLocationError {
    let sqlx::Error::Database(database) = &error else {
        return FactoryLocationError::StoreFailed;
    };
    match database.code().as_deref() {
        Some("23505") => FactoryLocationError::DuplicateName,
        Some("23503") => FactoryLocationError::InvalidApparatus,
        _ => FactoryLocationError::StoreFailed,
    }
}
