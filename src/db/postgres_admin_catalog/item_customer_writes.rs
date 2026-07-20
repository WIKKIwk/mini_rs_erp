use crate::core::admin::item_customer_policy::FINISHED_GOODS_CUSTOMER_REQUIRED;
use crate::core::admin::models::AdminItemGroup;
use crate::core::admin::ports::AdminPortError;
use crate::core::werka::models::SupplierItem;
use crate::db::postgres_admin_catalog::PostgresAdminCatalogStore;

use super::customer_policy::{
    customerless_item_codes, customerless_items_in_subtree, group_requires_customer,
    lock_item_customer_policy,
};
use super::rows::blank_default;

impl PostgresAdminCatalogStore {
    pub(super) async fn insert_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        self.insert_item_with_customer(code, name, uom, item_group, None)
            .await
    }

    pub(super) async fn insert_item_with_customer(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
        customer_ref: Option<&str>,
    ) -> Result<SupplierItem, AdminPortError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item code is required".to_string(),
            ));
        }
        let name = blank_default(name, code);
        let uom = blank_default(uom, "Kg");
        let group = blank_default(item_group, "All Item Groups");
        let payload = serde_json::json!({
            "code": code,
            "name": name,
            "uom": uom,
            "item_group": group,
        });

        self.ensure_item_group(&group).await?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        lock_item_customer_policy(&mut transaction).await?;
        let customer_ref = customer_ref
            .map(str::trim)
            .filter(|customer_ref| !customer_ref.is_empty());
        if group_requires_customer(&mut transaction, &group).await? && customer_ref.is_none() {
            return Err(AdminPortError::InvalidInput(
                FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
            ));
        }
        if let Some(customer_ref) = customer_ref {
            let customer_exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM mini_customers WHERE ref = $1)",
            )
            .bind(customer_ref)
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
            if !customer_exists {
                return Err(AdminPortError::NotFound);
            }
        }
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended(lower($1::text), 0))")
            .bind(code)
            .execute(&mut *transaction)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        let duplicate = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM mini_items WHERE lower(code) = lower($1))",
        )
        .bind(code)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        if duplicate {
            return Err(AdminPortError::InvalidInput(
                "item code already exists".to_string(),
            ));
        }
        let affected = sqlx::query(
            "INSERT INTO mini_items
                (code, name, uom, item_group, payload_json, updated_at)
             VALUES ($1, $2, $3, $4, $5, now())
             ON CONFLICT (code) DO NOTHING",
        )
        .bind(code)
        .bind(&name)
        .bind(&uom)
        .bind(&group)
        .bind(payload)
        .execute(&mut *transaction)
        .await
        .map_err(super::helpers::map_item_identity_write_error)?
        .rows_affected();
        if affected == 0 {
            return Err(AdminPortError::InvalidInput(
                "item code already exists".to_string(),
            ));
        }
        if let Some(customer_ref) = customer_ref {
            sqlx::query(
                "INSERT INTO mini_customer_items (customer_ref, item_code) VALUES ($1, $2)",
            )
            .bind(customer_ref)
            .bind(code)
            .execute(&mut *transaction)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        }
        transaction
            .commit()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;

        Ok(SupplierItem {
            code: code.to_string(),
            name,
            uom,
            warehouse: String::new(),
            item_group: group,
            customer_names: Vec::new(),
        })
    }

    pub(super) async fn update_item_groups_bulk_atomic(
        &self,
        item_codes: &[String],
        item_group: &str,
    ) -> Result<Vec<String>, AdminPortError> {
        let normalized_codes = item_codes
            .iter()
            .map(|code| code.trim().to_ascii_lowercase())
            .filter(|code| !code.is_empty())
            .collect::<Vec<_>>();
        if normalized_codes.is_empty() {
            return Ok(Vec::new());
        }
        let item_group = item_group.trim();
        self.ensure_item_group(item_group).await?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        lock_item_customer_policy(&mut transaction).await?;
        let stored_codes = sqlx::query_scalar::<_, String>(
            "SELECT code
             FROM mini_items
             WHERE lower(code) = ANY($1)
             ORDER BY lower(code)
             FOR UPDATE",
        )
        .bind(&normalized_codes)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        if group_requires_customer(&mut transaction, item_group).await? {
            let customerless = customerless_item_codes(&mut transaction, &stored_codes).await?;
            if !customerless.is_empty() {
                return Err(AdminPortError::InvalidInput(
                    FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
                ));
            }
        }
        if !stored_codes.is_empty() {
            sqlx::query(
                "UPDATE mini_items
                 SET item_group = $2,
                     payload_json = jsonb_set(
                         payload_json,
                         '{item_group}',
                         to_jsonb($2::text),
                         true
                     ),
                     updated_at = now()
                 WHERE code = ANY($1)",
            )
            .bind(&stored_codes)
            .bind(item_group)
            .execute(&mut *transaction)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        }
        transaction
            .commit()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        Ok(stored_codes)
    }

    pub(super) async fn upsert_item_group(
        &self,
        group: &AdminItemGroup,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let name = group.item_group_name.trim();
        if name.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item group name is required".to_string(),
            ));
        }
        let parent = group.parent_item_group.trim();
        let parent_group = if name == "All Item Groups" || parent.is_empty() {
            None
        } else {
            Some(parent)
        };
        let payload = serde_json::to_value(group).map_err(|_| AdminPortError::LookupFailed)?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        lock_item_customer_policy(&mut transaction).await?;
        let required_before = group_requires_customer(&mut transaction, name).await?;
        sqlx::query(
            "INSERT INTO mini_item_groups
                (name, parent_item_group, is_group, payload_json, updated_at)
             VALUES ($1, $2, $3, $4, now())
             ON CONFLICT (name) DO UPDATE SET
               parent_item_group = excluded.parent_item_group,
               is_group = excluded.is_group,
               payload_json = excluded.payload_json,
               updated_at = excluded.updated_at",
        )
        .bind(name)
        .bind(parent_group)
        .bind(group.is_group)
        .bind(payload)
        .execute(&mut *transaction)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        let required_after = group_requires_customer(&mut transaction, name).await?;
        if !required_before && required_after {
            let customerless = customerless_items_in_subtree(&mut transaction, name).await?;
            if !customerless.is_empty() {
                return Err(AdminPortError::InvalidInput(
                    FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
                ));
            }
        }
        transaction
            .commit()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;

        Ok(AdminItemGroup {
            name: name.to_string(),
            item_group_name: name.to_string(),
            parent_item_group: parent_group.unwrap_or("").to_string(),
            is_group: group.is_group,
        })
    }
}
