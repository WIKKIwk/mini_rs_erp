use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use crate::core::apparatus_groups::{ApparatusCatalogEntry, ApparatusSource};
use crate::core::factory_locations::{
    FactoryLocation, FactoryLocationError, FactoryLocationStorePort,
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
        apparatus: &[ApparatusCatalogEntry],
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
        apparatus: &[ApparatusCatalogEntry],
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
    apparatus: &[ApparatusCatalogEntry],
) -> Result<(), FactoryLocationError> {
    for item in apparatus {
        sqlx::query(
            "INSERT INTO mini_factory_location_apparatus_links (location_id, apparatus_id)
             VALUES ($1, $2)",
        )
        .bind(location_id)
        .bind(&item.id)
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
        "SELECT links.location_id, apparatus.id, apparatus.name, apparatus.kind,
                COALESCE((apparatus.payload_json ->> 'sort_order')::BIGINT, 10000)
                    AS sort_order
         FROM mini_factory_location_apparatus_links links
         JOIN mini_apparatus apparatus ON apparatus.id = links.apparatus_id
         WHERE ($1::TEXT IS NULL OR links.location_id = $1)
         ORDER BY links.location_id,
                  CASE WHEN apparatus.kind = 'default' THEN 0 ELSE 1 END,
                  COALESCE((apparatus.payload_json ->> 'sort_order')::BIGINT, 10000),
                  lower(apparatus.name) ASC",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(map_sql_error)?;

    let mut apparatus_by_location = BTreeMap::<String, Vec<ApparatusCatalogEntry>>::new();
    for row in link_rows {
        let kind: String = row.get("kind");
        apparatus_by_location
            .entry(row.get("location_id"))
            .or_default()
            .push(ApparatusCatalogEntry {
                id: row.get("id"),
                name: row.get("name"),
                source: if kind == "default" {
                    ApparatusSource::Default
                } else {
                    ApparatusSource::Custom
                },
                sort_order: row.get::<i64, _>("sort_order").max(0) as usize,
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
