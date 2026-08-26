use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use crate::core::apparatus_collections::{
    ApparatusCollection, ApparatusCollectionError, ApparatusCollectionStorePort,
};
use crate::core::apparatus_standard::ApparatusId;

#[derive(Clone)]
pub struct PostgresApparatusCollectionStore {
    pool: PgPool,
}

impl PostgresApparatusCollectionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ApparatusCollectionStorePort for PostgresApparatusCollectionStore {
    async fn list(&self) -> Result<Vec<ApparatusCollection>, ApparatusCollectionError> {
        let collection_rows = sqlx::query(
            "SELECT id, name, revision
             FROM mini_apparatus_collections
             ORDER BY lower(name) ASC, id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_sql_error)?;
        let member_rows = sqlx::query(
            "SELECT collection_id, apparatus_id
             FROM mini_apparatus_collection_members
             ORDER BY collection_id ASC, position ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_sql_error)?;

        let mut members = BTreeMap::<String, Vec<ApparatusId>>::new();
        for row in member_rows {
            let apparatus_id = ApparatusId::new(row.get::<String, _>("apparatus_id"))
                .map_err(|_| ApparatusCollectionError::StoreFailed)?;
            members
                .entry(row.get("collection_id"))
                .or_default()
                .push(apparatus_id);
        }

        collection_rows
            .into_iter()
            .map(|row| {
                let id: String = row.get("id");
                let revision: i64 = row.get("revision");
                Ok(ApparatusCollection {
                    apparatus_ids: members.remove(&id).unwrap_or_default(),
                    id,
                    name: row.get("name"),
                    revision: revision
                        .try_into()
                        .map_err(|_| ApparatusCollectionError::StoreFailed)?,
                })
            })
            .collect()
    }

    async fn create(
        &self,
        id: &str,
        name: &str,
        apparatus_ids: &[ApparatusId],
    ) -> Result<ApparatusCollection, ApparatusCollectionError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ApparatusCollectionError::StoreFailed)?;
        sqlx::query(
            "INSERT INTO mini_apparatus_collections (id, name, revision)
             VALUES ($1, $2, 1)",
        )
        .bind(id)
        .bind(name)
        .execute(&mut *tx)
        .await
        .map_err(map_sql_error)?;
        insert_members(&mut tx, id, apparatus_ids).await?;
        tx.commit()
            .await
            .map_err(|_| ApparatusCollectionError::StoreFailed)?;
        Ok(ApparatusCollection {
            id: id.to_string(),
            name: name.to_string(),
            apparatus_ids: apparatus_ids.to_vec(),
            revision: 1,
        })
    }

    async fn update(
        &self,
        id: &str,
        expected_revision: u64,
        name: &str,
        apparatus_ids: &[ApparatusId],
    ) -> Result<ApparatusCollection, ApparatusCollectionError> {
        let expected_revision = i64::try_from(expected_revision)
            .map_err(|_| ApparatusCollectionError::InvalidRevision)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ApparatusCollectionError::StoreFailed)?;
        let row = sqlx::query(
            "UPDATE mini_apparatus_collections
             SET name = $3,
                 revision = revision + 1,
                 updated_at = now()
             WHERE id = $1 AND revision = $2
             RETURNING revision",
        )
        .bind(id)
        .bind(expected_revision)
        .bind(name)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sql_error)?;
        let Some(row) = row else {
            return Err(revision_miss(&mut tx, id).await?);
        };
        sqlx::query(
            "DELETE FROM mini_apparatus_collection_members
             WHERE collection_id = $1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(map_sql_error)?;
        insert_members(&mut tx, id, apparatus_ids).await?;
        tx.commit()
            .await
            .map_err(|_| ApparatusCollectionError::StoreFailed)?;
        let revision: i64 = row.get("revision");
        Ok(ApparatusCollection {
            id: id.to_string(),
            name: name.to_string(),
            apparatus_ids: apparatus_ids.to_vec(),
            revision: revision
                .try_into()
                .map_err(|_| ApparatusCollectionError::StoreFailed)?,
        })
    }

    async fn delete(
        &self,
        id: &str,
        expected_revision: u64,
    ) -> Result<(), ApparatusCollectionError> {
        let expected_revision = i64::try_from(expected_revision)
            .map_err(|_| ApparatusCollectionError::InvalidRevision)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ApparatusCollectionError::StoreFailed)?;
        let result = sqlx::query(
            "DELETE FROM mini_apparatus_collections
             WHERE id = $1 AND revision = $2",
        )
        .bind(id)
        .bind(expected_revision)
        .execute(&mut *tx)
        .await
        .map_err(map_sql_error)?;
        if result.rows_affected() == 0 {
            return Err(revision_miss(&mut tx, id).await?);
        }
        tx.commit()
            .await
            .map_err(|_| ApparatusCollectionError::StoreFailed)?;
        Ok(())
    }
}

async fn insert_members(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    collection_id: &str,
    apparatus_ids: &[ApparatusId],
) -> Result<(), ApparatusCollectionError> {
    for (position, apparatus_id) in apparatus_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO mini_apparatus_collection_members
                (collection_id, apparatus_id, position)
             VALUES ($1, $2, $3)",
        )
        .bind(collection_id)
        .bind(apparatus_id.as_str())
        .bind(i32::try_from(position).map_err(|_| ApparatusCollectionError::StoreFailed)?)
        .execute(&mut **tx)
        .await
        .map_err(map_sql_error)?;
    }
    Ok(())
}

async fn revision_miss(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: &str,
) -> Result<ApparatusCollectionError, ApparatusCollectionError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM mini_apparatus_collections WHERE id = $1)",
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_sql_error)?;
    Ok(if exists {
        ApparatusCollectionError::RevisionConflict
    } else {
        ApparatusCollectionError::NotFound
    })
}

fn map_sql_error(error: sqlx::Error) -> ApparatusCollectionError {
    let sqlx::Error::Database(database) = &error else {
        return ApparatusCollectionError::StoreFailed;
    };
    match database.code().as_deref() {
        Some("23505") => ApparatusCollectionError::DuplicateName,
        Some("23503") => ApparatusCollectionError::InvalidApparatus,
        _ => ApparatusCollectionError::StoreFailed,
    }
}
