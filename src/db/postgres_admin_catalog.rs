use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::admin::item_customer_policy::FINISHED_GOODS_CUSTOMER_REQUIRED;
use crate::core::admin::models::{
    AdminDirectoryEntry, AdminItemDetail, AdminItemGroup, AdminWarehouse,
};
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminWritePort};
use crate::core::werka::models::SupplierItem;

pub(crate) mod customer_policy;
mod helpers;
mod item_delete_safety;
mod rows;

use self::customer_policy::{customerless_items_in_subtree, lock_item_customer_policy};
use self::rows::{ItemGroupRow, ItemRow};

#[derive(Clone)]
pub struct PostgresAdminCatalogStore {
    pool: PgPool,
}

impl PostgresAdminCatalogStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
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
            "SELECT code, name, uom, item_group
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
            "SELECT code, name, uom, item_group
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
            "SELECT code, name, uom, item_group
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

    async fn item_detail(&self, item_code: &str) -> Result<AdminItemDetail, AdminPortError> {
        self.load_item_detail(item_code).await
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
        let query = query.trim().to_lowercase();
        let needle = format!("%{query}%");
        let parent = parent.trim().to_lowercase();
        sqlx::query_as::<_, (String, String, bool, String)>(
            "SELECT name, company, is_group, parent_warehouse
             FROM mini_warehouses
             WHERE ($1 = '' OR lower(name) LIKE $2)
               AND ($3 = '' OR lower(parent_warehouse) = $3)
             ORDER BY lower(name)
             LIMIT $4",
        )
        .bind(query)
        .bind(needle)
        .bind(parent)
        .bind(limit.clamp(1, 500) as i64)
        .fetch_all(&self.pool)
        .await
        .map(|rows| {
            rows.into_iter()
                .map(
                    |(warehouse, company, is_group, parent_warehouse)| AdminWarehouse {
                        warehouse,
                        company,
                        is_group,
                        parent_warehouse,
                    },
                )
                .collect()
        })
        .map_err(|_| AdminPortError::LookupFailed)
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        sqlx::query_as::<_, ItemGroupRow>(
            "SELECT name, COALESCE(parent_item_group, '') AS parent_item_group, is_group
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
        self.insert_item(code, name, uom, item_group).await
    }

    async fn create_item_with_customer(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
        customer_ref: Option<&str>,
    ) -> Result<SupplierItem, AdminPortError> {
        self.insert_item_with_customer(code, name, uom, item_group, customer_ref)
            .await
    }

    async fn update_item(
        &self,
        original_code: &str,
        code: &str,
        name: &str,
    ) -> Result<AdminItemDetail, AdminPortError> {
        self.update_item_identity(original_code, code, name).await
    }

    async fn delete_item(&self, code: &str) -> Result<(), AdminPortError> {
        self.delete_item_safely(code).await
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
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        lock_item_customer_policy(&mut transaction).await?;
        let affected = sqlx::query(
            "UPDATE mini_item_groups
             SET parent_item_group = $2, updated_at = now()
             WHERE name = $1",
        )
        .bind(name.trim())
        .bind(parent.trim())
        .execute(&mut *transaction)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?
        .rows_affected();
        if affected == 0 {
            return Err(AdminPortError::NotFound);
        }
        let customerless = customerless_items_in_subtree(&mut transaction, name).await?;
        if !customerless.is_empty() {
            return Err(AdminPortError::InvalidInput(
                FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
            ));
        }
        transaction
            .commit()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
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
        let updated = self
            .update_item_groups_bulk_atomic(&[item_code.trim().to_string()], item_group)
            .await?;
        if updated.is_empty() {
            return Err(AdminPortError::NotFound);
        }
        Ok(())
    }

    async fn update_item_groups_bulk(
        &self,
        item_codes: &[String],
        item_group: &str,
    ) -> Result<Vec<String>, AdminPortError> {
        self.update_item_groups_bulk_atomic(item_codes, item_group)
            .await
    }
}
