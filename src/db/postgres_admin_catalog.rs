use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup, AdminWarehouse};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminWritePort};
use crate::core::werka::models::SupplierItem;

#[derive(Clone)]
pub struct PostgresAdminCatalogStore {
    pool: PgPool,
}

impl PostgresAdminCatalogStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn seed_from_read_port(
        &self,
        source: &(dyn AdminReadPort + Send + Sync),
    ) -> Result<(), AdminPortError> {
        for group in source.item_group_tree().await? {
            self.upsert_item_group(&group).await?;
        }

        let mut offset = 0usize;
        loop {
            let items = source.items_page("", 500, offset).await?;
            if items.is_empty() {
                break;
            }
            for item in &items {
                self.upsert_item(item).await?;
            }
            offset += items.len();
            if items.len() < 500 {
                break;
            }
        }
        Ok(())
    }

    async fn upsert_item(&self, item: &SupplierItem) -> Result<SupplierItem, AdminPortError> {
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

    async fn upsert_item_group(
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
        .bind(parent)
        .bind(group.is_group)
        .bind(payload)
        .execute(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;

        Ok(AdminItemGroup {
            name: name.to_string(),
            item_group_name: name.to_string(),
            parent_item_group: parent.to_string(),
            is_group: group.is_group,
        })
    }

    async fn ensure_item_group(&self, name: &str) -> Result<(), AdminPortError> {
        let name = name.trim();
        if name.is_empty() {
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

#[async_trait]
impl AdminReadPort for PostgresAdminCatalogStore {
    async fn suppliers_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn supplier_by_ref(&self, _ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::NotFound)
    }

    async fn customers_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn customer_by_ref(&self, _ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::NotFound)
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let needle = format!("%{}%", query.trim().to_lowercase());
        sqlx::query_as::<_, ItemRow>(
            "SELECT code, name, uom, warehouse, item_group
             FROM mini_items
             WHERE $1 = '%%'
                OR lower(code) LIKE $1
                OR lower(name) LIKE $1
                OR lower(item_group) LIKE $1
             ORDER BY lower(code)
             LIMIT $2 OFFSET $3",
        )
        .bind(needle)
        .bind(limit.min(500) as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(ItemRow::into_item).collect())
        .map_err(|_| AdminPortError::LookupFailed)
    }

    async fn items_page_by_group(
        &self,
        group: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let group = group.trim();
        if group.is_empty() {
            return self.items_page(query, limit, offset).await;
        }
        let needle = format!("%{}%", query.trim().to_lowercase());
        sqlx::query_as::<_, ItemRow>(
            "SELECT code, name, uom, warehouse, item_group
             FROM mini_items
             WHERE lower(item_group) = lower($1)
               AND (
                    $2 = '%%'
                    OR lower(code) LIKE $2
                    OR lower(name) LIKE $2
                    OR lower(item_group) LIKE $2
               )
             ORDER BY lower(code)
             LIMIT $3 OFFSET $4",
        )
        .bind(group)
        .bind(needle)
        .bind(limit.min(500) as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(ItemRow::into_item).collect())
        .map_err(|_| AdminPortError::LookupFailed)
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let codes = item_codes
            .iter()
            .map(|code| code.trim().to_lowercase())
            .filter(|code| !code.is_empty())
            .collect::<Vec<_>>();
        if codes.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query_as::<_, ItemRow>(
            "SELECT code, name, uom, warehouse, item_group
             FROM mini_items
             WHERE lower(code) = ANY($1)
             ORDER BY lower(code)",
        )
        .bind(codes)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(ItemRow::into_item).collect())
        .map_err(|_| AdminPortError::LookupFailed)
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        let needle = format!("%{}%", query.trim().to_lowercase());
        sqlx::query_scalar::<_, String>(
            "SELECT name
             FROM mini_item_groups
             WHERE $1 = '%%' OR lower(name) LIKE $1
             ORDER BY lower(name)
             LIMIT $2",
        )
        .bind(needle)
        .bind(limit.min(500) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)
    }

    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, AdminPortError> {
        if !parent.trim().is_empty() {
            return Ok(Vec::new());
        }
        let needle = format!("%{}%", query.trim().to_lowercase());
        sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT warehouse
             FROM mini_items
             WHERE btrim(warehouse) <> ''
               AND ($1 = '%%' OR lower(warehouse) LIKE $1)
             ORDER BY warehouse
             LIMIT $2",
        )
        .bind(needle)
        .bind(limit.min(500) as i64)
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(|warehouse| AdminWarehouse {
                    warehouse,
                    company: String::new(),
                    is_group: false,
                    parent_warehouse: String::new(),
                })
                .collect()
        })
        .map_err(|_| AdminPortError::LookupFailed)
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        sqlx::query_as::<_, ItemGroupRow>(
            "SELECT name, parent_item_group, is_group
             FROM mini_item_groups
             ORDER BY lower(name)",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(ItemGroupRow::into_group).collect())
        .map_err(|_| AdminPortError::LookupFailed)
    }

    async fn assigned_supplier_items(
        &self,
        _supplier_ref: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn customer_items(
        &self,
        _customer_ref: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl AdminWritePort for PostgresAdminCatalogStore {
    async fn create_supplier(
        &self,
        _name: &str,
        _phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn update_supplier_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn assign_supplier_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn unassign_supplier_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn create_customer(
        &self,
        _name: &str,
        _phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn update_customer_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn update_customer_code(&self, _ref_: &str, _code: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn assign_customer_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn unassign_customer_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        self.upsert_item(&SupplierItem {
            code: code.trim().to_string(),
            name: name.trim().to_string(),
            uom: uom.trim().to_string(),
            warehouse: String::new(),
            item_group: item_group.trim().to_string(),
        })
        .await
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        self.upsert_item_group(&AdminItemGroup {
            name: name.trim().to_string(),
            item_group_name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group,
        })
        .await
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let affected = sqlx::query(
            "UPDATE mini_item_groups
             SET parent_item_group = $2, updated_at = now()
             WHERE name = $1",
        )
        .bind(name.trim())
        .bind(parent.trim())
        .execute(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?
        .rows_affected();
        if affected == 0 {
            return Err(AdminPortError::NotFound);
        }
        Ok(AdminItemGroup {
            name: name.trim().to_string(),
            item_group_name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group: true,
        })
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        self.ensure_item_group(item_group).await?;
        let affected = sqlx::query(
            "UPDATE mini_items
             SET item_group = $2,
                 payload_json = jsonb_set(payload_json, '{item_group}', to_jsonb($2::text), true),
                 updated_at = now()
             WHERE code = $1",
        )
        .bind(item_code.trim())
        .bind(item_group.trim())
        .execute(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?
        .rows_affected();
        if affected == 0 {
            return Err(AdminPortError::NotFound);
        }
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct ItemRow {
    code: String,
    name: String,
    uom: String,
    warehouse: String,
    item_group: String,
}

impl ItemRow {
    fn into_item(self) -> SupplierItem {
        SupplierItem {
            code: self.code,
            name: self.name,
            uom: self.uom,
            warehouse: self.warehouse,
            item_group: self.item_group,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ItemGroupRow {
    name: String,
    parent_item_group: String,
    is_group: bool,
}

impl ItemGroupRow {
    fn into_group(self) -> AdminItemGroup {
        AdminItemGroup {
            name: self.name.clone(),
            item_group_name: self.name,
            parent_item_group: self.parent_item_group,
            is_group: self.is_group,
        }
    }
}

fn blank_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim().to_string()
    } else {
        value.to_string()
    }
}
