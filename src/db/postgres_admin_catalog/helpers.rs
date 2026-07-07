use crate::core::admin::models::AdminItemGroup;
use crate::core::admin::ports::AdminPortError;
use crate::core::werka::models::SupplierItem;
use crate::db::postgres_admin_catalog::PostgresAdminCatalogStore;

use super::rows::blank_default;

impl PostgresAdminCatalogStore {
    pub(super) async fn upsert_item(
        &self,
        item: &SupplierItem,
    ) -> Result<SupplierItem, AdminPortError> {
        let code = item.code.trim();
        if code.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item code is required".to_string(),
            ));
        }
        let name = blank_default(&item.name, code);
        let uom = blank_default(&item.uom, "Kg");
        let group = blank_default(&item.item_group, "All Item Groups");
        let payload = serde_json::to_value(SupplierItem {
            code: code.to_string(),
            name: name.clone(),
            uom: uom.clone(),
            warehouse: item.warehouse.trim().to_string(),
            item_group: group.clone(),
        })
        .map_err(|_| AdminPortError::LookupFailed)?;

        self.ensure_item_group(&group).await?;
        sqlx::query(
            "INSERT INTO mini_items
                (code, name, uom, warehouse, item_group, payload_json, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT (code) DO UPDATE SET
               name = excluded.name,
               uom = excluded.uom,
               warehouse = excluded.warehouse,
               item_group = excluded.item_group,
               payload_json = excluded.payload_json,
               updated_at = excluded.updated_at",
        )
        .bind(code)
        .bind(&name)
        .bind(&uom)
        .bind(item.warehouse.trim())
        .bind(&group)
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;

        Ok(SupplierItem {
            code: code.to_string(),
            name,
            uom,
            warehouse: item.warehouse.trim().to_string(),
            item_group: group,
        })
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
        .execute(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;

        Ok(AdminItemGroup {
            name: name.to_string(),
            item_group_name: name.to_string(),
            parent_item_group: parent_group.unwrap_or("").to_string(),
            is_group: group.is_group,
        })
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
