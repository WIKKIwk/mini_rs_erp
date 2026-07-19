use sqlx::{Postgres, Transaction};

use crate::core::admin::item_customer_policy::item_group_requires_customer;
use crate::core::admin::models::AdminItemDetail;
use crate::core::admin::ports::{AdminPortError, AdminReadPort};
use crate::db::postgres_admin_catalog::PostgresAdminCatalogStore;

use super::item_delete_safety::{item_delete_blocker, map_item_delete_write_error};
use super::rows::{ItemCustomerRow, ItemDetailRow};

impl PostgresAdminCatalogStore {
    pub(super) async fn load_item_detail(
        &self,
        item_code: &str,
    ) -> Result<AdminItemDetail, AdminPortError> {
        let item_code = item_code.trim();
        if item_code.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item code is required".to_string(),
            ));
        }
        let row = sqlx::query_as::<_, ItemDetailRow>(
            r#"
            SELECT items.code, items.name, items.uom, items.item_group,
                   EXTRACT(EPOCH FROM items.created_at)::bigint AS created_at_unix,
                   EXTRACT(EPOCH FROM items.updated_at)::bigint AS updated_at_unix
            FROM mini_items items
            WHERE items.code = $1
            "#,
        )
        .bind(item_code)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?
        .ok_or(AdminPortError::NotFound)?;
        let groups = AdminReadPort::item_group_tree(self).await?;
        let is_finished_goods = item_group_requires_customer(row.item_group(), &groups);
        let customers = sqlx::query_as::<_, ItemCustomerRow>(
            "SELECT customers.ref AS customer_ref, customers.name, customers.phone
             FROM mini_customer_items assignments
             JOIN mini_customers customers ON customers.ref = assignments.customer_ref
             WHERE assignments.item_code = $1
             ORDER BY lower(customers.name), customers.ref",
        )
        .bind(item_code)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?
        .into_iter()
        .map(ItemCustomerRow::into_customer)
        .collect();
        Ok(row.into_detail(customers, is_finished_goods))
    }

    pub(super) async fn update_item_identity(
        &self,
        original_code: &str,
        code: &str,
        name: &str,
    ) -> Result<AdminItemDetail, AdminPortError> {
        let original_code = original_code.trim();
        let code = code.trim();
        let name = name.trim();
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        let duplicate = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                 SELECT 1 FROM mini_items
                 WHERE lower(code) = lower($1) AND code <> $2
             )",
        )
        .bind(code)
        .bind(original_code)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        if duplicate {
            return Err(AdminPortError::InvalidInput(
                "item code already exists".to_string(),
            ));
        }
        let affected = sqlx::query(
            "UPDATE mini_items
             SET code = $2,
                 name = $3,
                 payload_json = jsonb_set(
                     jsonb_set(payload_json, '{code}', to_jsonb($2::text), true),
                     '{name}', to_jsonb($3::text), true
                 ),
                 updated_at = now()
             WHERE code = $1",
        )
        .bind(original_code)
        .bind(code)
        .bind(name)
        .execute(&mut *transaction)
        .await
        .map_err(map_item_identity_write_error)?
        .rows_affected();
        if affected == 0 {
            return Err(AdminPortError::NotFound);
        }
        update_operational_item_projections(&mut transaction, original_code, code, name).await?;
        transaction
            .commit()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        self.load_item_detail(code).await
    }

    pub(super) async fn delete_item_safely(&self, code: &str) -> Result<(), AdminPortError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item code is required".to_string(),
            ));
        }
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended(lower($1::text), 0))")
            .bind(code)
            .execute(&mut *transaction)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        let (stored_code, stored_name) = sqlx::query_as::<_, (String, String)>(
            "SELECT code, name
             FROM mini_items
             WHERE lower(code) = lower($1)
             ORDER BY (code = $1) DESC
             LIMIT 1
             FOR UPDATE",
        )
        .bind(code)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?
        .ok_or(AdminPortError::NotFound)?;
        let blocker = item_delete_blocker(&mut transaction, &stored_code, &stored_name).await?;
        if !blocker.is_empty() {
            return Err(AdminPortError::InvalidInput(blocker));
        }
        let affected = sqlx::query("DELETE FROM mini_items WHERE code = $1")
            .bind(&stored_code)
            .execute(&mut *transaction)
            .await
            .map_err(map_item_delete_write_error)?
            .rows_affected();
        if affected == 0 {
            return Err(AdminPortError::NotFound);
        }
        transaction
            .commit()
            .await
            .map_err(|_| AdminPortError::LookupFailed)
    }

    pub(super) async fn ensure_item_group(&self, name: &str) -> Result<(), AdminPortError> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO mini_item_groups
                (name, parent_item_group, is_group, payload_json, updated_at)
             VALUES ('All Item Groups', NULL, true, $1, now())
             ON CONFLICT (name) DO NOTHING",
        )
        .bind(serde_json::json!({
            "name": "All Item Groups",
            "item_group_name": "All Item Groups",
            "parent_item_group": null,
            "is_group": true
        }))
        .execute(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        if name == "All Item Groups" {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO mini_item_groups
                (name, parent_item_group, is_group, payload_json, updated_at)
             VALUES ($1, 'All Item Groups', true, $2, now())
             ON CONFLICT (name) DO NOTHING",
        )
        .bind(name)
        .bind(serde_json::json!({
            "name": name,
            "item_group_name": name,
            "parent_item_group": "All Item Groups",
            "is_group": true
        }))
        .execute(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        Ok(())
    }
}

async fn update_operational_item_projections(
    transaction: &mut Transaction<'_, Postgres>,
    original_code: &str,
    code: &str,
    name: &str,
) -> Result<(), AdminPortError> {
    sqlx::query(
        "UPDATE mini_raw_material_assignments
         SET payload_json = jsonb_set(
                 jsonb_set(payload_json, '{item_code}', to_jsonb($1::text), true),
                 '{item_name}', to_jsonb($2::text), true
             ),
             updated_at = now()
         WHERE item_code = $1",
    )
    .bind(code)
    .bind(name)
    .execute(&mut **transaction)
    .await
    .map_err(|_| AdminPortError::LookupFailed)?;

    for statement in [
        "UPDATE mini_gscale_receipts
         SET item_code = $2,
             payload_json = jsonb_set(jsonb_set(payload_json, '{item_code}', to_jsonb($2::text), true), '{item_name}', to_jsonb($3::text), true),
             updated_at = now()
         WHERE item_code = $1 AND status = 'draft'",
        "UPDATE mini_raw_material_stock
         SET item_code = $2, item_name = $3,
             payload_json = jsonb_set(jsonb_set(payload_json, '{item_code}', to_jsonb($2::text), true), '{item_name}', to_jsonb($3::text), true),
             updated_at = now()
         WHERE item_code = $1",
        "UPDATE mini_finished_goods_stock
         SET item_code = $2, item_name = $3,
             payload_json = jsonb_set(jsonb_set(payload_json, '{item_code}', to_jsonb($2::text), true), '{item_name}', to_jsonb($3::text), true),
             updated_at = now()
         WHERE item_code = $1",
        "UPDATE mini_qolip_locations
         SET item_code = $2, item_name = $3,
             payload_json = jsonb_set(jsonb_set(payload_json, '{item_code}', to_jsonb($2::text), true), '{item_name}', to_jsonb($3::text), true),
             updated_at = now()
         WHERE item_code = $1",
        "UPDATE mini_qolip_product_specs
         SET item_code = $2, item_name = $3,
             payload_json = jsonb_set(jsonb_set(payload_json, '{item_code}', to_jsonb($2::text), true), '{item_name}', to_jsonb($3::text), true),
             updated_at = now()
         WHERE item_code = $1",
        "UPDATE mini_qolip_checkouts
         SET item_code = $2, item_name = $3,
             payload_json = jsonb_set(jsonb_set(payload_json, '{item_code}', to_jsonb($2::text), true), '{item_name}', to_jsonb($3::text), true),
             updated_at = now()
         WHERE item_code = $1 AND status = 'open'",
    ] {
        sqlx::query(statement)
            .bind(original_code)
            .bind(code)
            .bind(name)
            .execute(&mut **transaction)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
    }

    sqlx::query(
        "UPDATE mini_quick_order_templates
         SET item_code = $2, product_name = $3,
             payload_json = jsonb_set(jsonb_set(payload_json, '{item_code}', to_jsonb($2::text), true), '{product_name}', to_jsonb($3::text), true)
         WHERE item_code = $1",
    )
    .bind(original_code)
    .bind(code)
    .bind(name)
    .execute(&mut **transaction)
    .await
    .map_err(|_| AdminPortError::LookupFailed)?;

    sqlx::query(
        "UPDATE mini_rps_batches
         SET item_code = $2,
             payload_json = jsonb_set(payload_json, '{item_code}', to_jsonb($2::text), true),
             updated_at = now()
         WHERE item_code = $1",
    )
    .bind(original_code)
    .bind(code)
    .execute(&mut **transaction)
    .await
    .map_err(|_| AdminPortError::LookupFailed)?;
    Ok(())
}

pub(super) fn map_item_identity_write_error(error: sqlx::Error) -> AdminPortError {
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| code == "23505")
    {
        AdminPortError::InvalidInput("item code already exists".to_string())
    } else {
        AdminPortError::LookupFailed
    }
}
