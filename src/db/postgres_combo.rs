//! Additive combo catalog storage. No live order or child SKU is rewritten.
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::combo::{
    ComboCatalogEntry, ComboError, ComboReference, layout_fingerprint, normalize_name,
};

#[cfg(test)]
mod tests;
mod validation;
mod writes;

#[derive(Clone)]
pub struct PostgresComboStore {
    pool: PgPool,
}

impl PostgresComboStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// An omitted master ID can only reuse an identical named definition.
    /// Changes to an explicit existing master create an independent catalog SKU.
    pub async fn save(
        &self,
        name: &str,
        combo_id: Option<&str>,
        template: &CalculateOrderTemplate,
    ) -> Result<ComboReference, ComboError> {
        let name = normalize_name(name)?;
        let fingerprint = layout_fingerprint(template)?;
        let requested_id = combo_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .or_else(|| template.combo.as_ref().map(|combo| combo.id.as_str()));
        let mut tx = self.pool.begin().await.map_err(store_error)?;
        validation::lock_catalog_policy(&mut tx).await?;
        validation::components(&mut tx, template).await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!(
                "combo:{}:{}",
                template.customer_ref.trim(),
                name.to_lowercase()
            ))
            .execute(&mut *tx)
            .await
            .map_err(store_error)?;
        if let Some(incoming) = &template.combo {
            if requested_id != Some(incoming.id.as_str()) {
                return Err(ComboError::ReferenceMismatch);
            }
            let existing = get_tx(&mut tx, &incoming.item_code)
                .await?
                .ok_or(ComboError::NotFound)?;
            if existing.combo != *incoming {
                return Err(ComboError::ReferenceMismatch);
            }
        }
        let master = match requested_id {
            Some(id) => sqlx::query_as::<_, (String, String, String)>(
                "SELECT id, customer_ref, name FROM mini_combo_masters WHERE id=$1",
            )
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(store_error)?,
            None => sqlx::query_as::<_, (String, String, String)>(
                "SELECT id, customer_ref, name FROM mini_combo_masters
                 WHERE customer_ref=$1 AND normalized_name=$2",
            )
            .bind(template.customer_ref.trim())
            .bind(name.to_lowercase())
            .fetch_optional(&mut *tx)
            .await
            .map_err(store_error)?,
        };
        let (id, master_name) = match master {
            Some((id, customer, existing_name)) => {
                if customer != template.customer_ref.trim()
                    || existing_name.to_lowercase() != name.to_lowercase()
                {
                    return Err(ComboError::NameConflict);
                }
                let same = sqlx::query_scalar::<_, serde_json::Value>(
                    "SELECT reference_json FROM mini_combo_revisions
                     WHERE combo_id=$1 AND layout_fingerprint=$2",
                )
                .bind(&id)
                .bind(&fingerprint)
                .fetch_optional(&mut *tx)
                .await
                .map_err(store_error)?;
                if let Some(reference) = same {
                    let result =
                        serde_json::from_value(reference).map_err(|_| ComboError::StoreFailed)?;
                    tx.commit().await.map_err(store_error)?;
                    return Ok(result);
                }
                if requested_id.is_none() {
                    return Err(ComboError::NameConflict);
                }
                (id, existing_name)
            }
            None if requested_id.is_some() => return Err(ComboError::NotFound),
            None => {
                let id = format!("combo-{:032x}", rand::random::<u128>());
                sqlx::query(
                    "INSERT INTO mini_combo_masters(id,customer_ref,name,normalized_name)
                    VALUES ($1,$2,$3,$4)",
                )
                .bind(&id)
                .bind(template.customer_ref.trim())
                .bind(&name)
                .bind(name.to_lowercase())
                .execute(&mut *tx)
                .await
                .map_err(store_error)?;
                (id, name)
            }
        };
        let revision: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(revision),0)+1 FROM mini_combo_revisions WHERE combo_id=$1",
        )
        .bind(&id)
        .fetch_one(&mut *tx)
        .await
        .map_err(store_error)?;
        let reference =
            writes::revision(&mut tx, id, master_name, revision, fingerprint, template).await?;
        tx.commit().await.map_err(store_error)?;
        Ok(reference)
    }

    pub async fn get(&self, item_code: &str) -> Result<Option<ComboCatalogEntry>, ComboError> {
        let mut tx = self.pool.begin().await.map_err(store_error)?;
        let entry = get_tx(&mut tx, item_code.trim()).await?;
        tx.commit().await.map_err(store_error)?;
        Ok(entry)
    }

    /// Validate before hydration, so forged primary fields cannot be normalized away.
    pub async fn validate(&self, template: &CalculateOrderTemplate) -> Result<(), ComboError> {
        let mut tx = self.pool.begin().await.map_err(store_error)?;
        validation::lock_catalog_policy(&mut tx).await?;
        if let Some(reference) = &template.combo {
            crate::core::combo::validate_reference(template)?;
            let stored = get_tx(&mut tx, &reference.item_code)
                .await?
                .ok_or(ComboError::NotFound)?;
            if stored.combo != *reference || stored.template.customer_ref != template.customer_ref {
                return Err(ComboError::ReferenceMismatch);
            }
            validation::components(&mut tx, template).await?;
        } else {
            validation::reject_combo_sku(&mut tx, &template.item_code).await?;
            for product in &template.products {
                validation::reject_combo_sku(&mut tx, &product.item_code).await?;
            }
        }
        tx.commit().await.map_err(store_error)
    }
}

async fn get_tx(
    tx: &mut Transaction<'_, Postgres>,
    item_code: &str,
) -> Result<Option<ComboCatalogEntry>, ComboError> {
    let row = sqlx::query_as::<_, (serde_json::Value, serde_json::Value)>(
        "SELECT reference_json, template_json FROM mini_combo_revisions WHERE item_code=$1",
    )
    .bind(item_code)
    .fetch_optional(&mut **tx)
    .await
    .map_err(store_error)?;
    row.map(|(reference, template)| {
        Ok(ComboCatalogEntry {
            combo: serde_json::from_value(reference).map_err(|_| ComboError::StoreFailed)?,
            template: serde_json::from_value(template).map_err(|_| ComboError::StoreFailed)?,
        })
    })
    .transpose()
}

fn store_error(error: sqlx::Error) -> ComboError {
    tracing::error!(%error, "combo catalog storage operation failed");
    ComboError::StoreFailed
}
