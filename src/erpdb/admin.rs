use async_trait::async_trait;
use sqlx::{MySql, QueryBuilder, query_as};

use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup, AdminWarehouse};
use crate::core::admin::ports::{AdminPortError, AdminReadPort};
use crate::core::werka::models::SupplierItem;
use crate::erpdb::reader::DirectDbReader;
use crate::erpdb::werka_suppliers::{clamp_limit, like_pattern};

#[async_trait]
impl AdminReadPort for DirectDbReader {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        let like = like_pattern(query);
        let rows = query_as::<_, AdminDirectoryRow>(ADMIN_SUPPLIERS_PAGE_SQL)
            .bind(query.trim())
            .bind(&like)
            .bind(&like)
            .bind(&like)
            .bind(clamp_limit(limit, 50, 500) as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(map_lookup_error)?;
        Ok(rows
            .into_iter()
            .map(AdminDirectoryRow::into_entry)
            .collect())
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        query_as::<_, AdminDirectoryRow>(ADMIN_SUPPLIER_BY_REF_SQL)
            .bind(ref_.trim())
            .fetch_one(&self.pool)
            .await
            .map(AdminDirectoryRow::into_entry)
            .map_err(map_not_found_error)
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        let like = like_pattern(query);
        let rows = query_as::<_, AdminDirectoryRow>(ADMIN_CUSTOMERS_PAGE_SQL)
            .bind(query.trim())
            .bind(&like)
            .bind(&like)
            .bind(&like)
            .bind(clamp_limit(limit, 50, 500) as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(map_lookup_error)?;
        Ok(rows
            .into_iter()
            .map(AdminDirectoryRow::into_entry)
            .collect())
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        query_as::<_, AdminDirectoryRow>(ADMIN_CUSTOMER_BY_REF_SQL)
            .bind(ref_.trim())
            .fetch_one(&self.pool)
            .await
            .map(AdminDirectoryRow::into_entry)
            .map_err(map_not_found_error)
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let like = like_pattern(query);
        let rows = query_as::<_, AdminItemRow>(ADMIN_ITEMS_PAGE_SQL)
            .bind(query.trim())
            .bind(&like)
            .bind(&like)
            .bind(clamp_limit(limit, 50, 500) as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(map_lookup_error)?;
        Ok(rows
            .into_iter()
            .map(|row| row.into_item(&self.default_warehouse))
            .collect())
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
        let like = like_pattern(query);
        let rows = query_as::<_, AdminItemRow>(ADMIN_ITEMS_BY_GROUP_PAGE_SQL)
            .bind(group)
            .bind(query.trim())
            .bind(&like)
            .bind(&like)
            .bind(clamp_limit(limit, 50, 500) as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(map_lookup_error)?;
        Ok(rows
            .into_iter()
            .map(|row| row.into_item(&self.default_warehouse))
            .collect())
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let codes = item_codes
            .iter()
            .map(|code| code.trim())
            .filter(|code| !code.is_empty())
            .take(500)
            .collect::<Vec<_>>();
        if codes.is_empty() {
            return Ok(Vec::new());
        }

        let mut builder = QueryBuilder::<MySql>::new(
            r#"
                SELECT
                    i.name AS item_code,
                    COALESCE(NULLIF(i.item_name, ''), i.name) AS item_name,
                    COALESCE(i.stock_uom, '') AS stock_uom,
                    COALESCE(i.item_group, '') AS item_group
                FROM tabItem i
                WHERE i.disabled = 0
                  AND i.is_stock_item = 1
                  AND i.name IN (
            "#,
        );
        let mut separated = builder.separated(", ");
        for code in codes {
            separated.push_bind(code);
        }
        separated.push_unseparated(") ORDER BY i.item_name ASC, i.name ASC");

        let rows = builder
            .build_query_as::<AdminItemRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(map_lookup_error)?;
        Ok(rows
            .into_iter()
            .map(|row| row.into_item(&self.default_warehouse))
            .collect())
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        let like = like_pattern(query);
        let rows = query_as::<_, AdminItemGroupRow>(ADMIN_ITEM_GROUPS_SQL)
            .bind(query.trim())
            .bind(&like)
            .bind(&like)
            .bind(clamp_limit(limit, 50, 500) as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(map_lookup_error)?;
        Ok(rows
            .into_iter()
            .map(|row| row.name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect())
    }

    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, AdminPortError> {
        let like = like_pattern(query);
        let rows = query_as::<_, AdminWarehouseRow>(ADMIN_WAREHOUSES_SQL)
            .bind(query.trim())
            .bind(&like)
            .bind(&like)
            .bind(parent.trim())
            .bind(parent.trim())
            .bind(clamp_limit(limit, 30, 500) as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(map_lookup_error)?;
        Ok(rows
            .into_iter()
            .map(AdminWarehouseRow::into_warehouse)
            .filter(|warehouse| !warehouse.warehouse.is_empty())
            .collect())
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        let rows = query_as::<_, AdminItemGroupRow>(ADMIN_ITEM_GROUP_TREE_SQL)
            .fetch_all(&self.pool)
            .await
            .map_err(map_lookup_error)?;
        Ok(rows
            .into_iter()
            .map(AdminItemGroupRow::into_group)
            .filter(|group| !group.name.is_empty())
            .collect())
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let rows = query_as::<_, AdminItemRow>(ADMIN_ASSIGNED_SUPPLIER_ITEMS_SQL)
            .bind(supplier_ref.trim())
            .bind(clamp_limit(limit, 200, 500) as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(map_lookup_error)?;
        Ok(rows
            .into_iter()
            .map(|row| row.into_item(&self.default_warehouse))
            .collect())
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let like = like_pattern(query);
        let rows = query_as::<_, AdminItemRow>(ADMIN_CUSTOMER_ITEMS_SQL)
            .bind(customer_ref.trim())
            .bind(query.trim())
            .bind(&like)
            .bind(&like)
            .bind(clamp_limit(limit, 200, 500) as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(map_lookup_error)?;
        Ok(rows
            .into_iter()
            .map(|row| row.into_item(&self.default_warehouse))
            .collect())
    }
}

#[derive(Debug, sqlx::FromRow)]
struct AdminDirectoryRow {
    #[sqlx(rename = "ref")]
    ref_: String,
    name: String,
    phone: String,
}

impl AdminDirectoryRow {
    fn into_entry(self) -> AdminDirectoryEntry {
        AdminDirectoryEntry {
            ref_: self.ref_,
            name: self.name,
            phone: self.phone,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct AdminItemRow {
    item_code: String,
    item_name: String,
    stock_uom: String,
    item_group: String,
}

impl AdminItemRow {
    fn into_item(self, default_warehouse: &str) -> SupplierItem {
        SupplierItem {
            code: self.item_code.trim().to_string(),
            name: self.item_name.trim().to_string(),
            uom: self.stock_uom.trim().to_string(),
            warehouse: default_warehouse.trim().to_string(),
            item_group: self.item_group.trim().to_string(),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct AdminItemGroupRow {
    name: String,
    item_group_name: String,
    parent_item_group: String,
    is_group: i32,
}

impl AdminItemGroupRow {
    fn into_group(self) -> AdminItemGroup {
        AdminItemGroup {
            name: self.name.trim().to_string(),
            item_group_name: blank_default(&self.item_group_name, &self.name),
            parent_item_group: self.parent_item_group.trim().to_string(),
            is_group: self.is_group != 0,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct AdminWarehouseRow {
    name: String,
    company: String,
    is_group: i32,
    parent_warehouse: String,
}

impl AdminWarehouseRow {
    fn into_warehouse(self) -> AdminWarehouse {
        AdminWarehouse {
            warehouse: self.name.trim().to_string(),
            company: self.company.trim().to_string(),
            is_group: self.is_group != 0,
            parent_warehouse: self.parent_warehouse.trim().to_string(),
        }
    }
}

fn map_lookup_error(_error: sqlx::Error) -> AdminPortError {
    AdminPortError::LookupFailed
}

fn blank_default(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

fn map_not_found_error(error: sqlx::Error) -> AdminPortError {
    match error {
        sqlx::Error::RowNotFound => AdminPortError::NotFound,
        _ => AdminPortError::LookupFailed,
    }
}

const ADMIN_SUPPLIERS_PAGE_SQL: &str = r#"
    SELECT
        s.name AS ref,
        COALESCE(NULLIF(s.supplier_name, ''), s.name) AS name,
        COALESCE(s.mobile_no, '') AS phone
    FROM tabSupplier s
    WHERE s.disabled = 0
      AND (? = '' OR s.name LIKE ? ESCAPE '\\' OR s.supplier_name LIKE ? ESCAPE '\\' OR COALESCE(s.mobile_no, '') LIKE ? ESCAPE '\\')
    ORDER BY s.modified DESC
    LIMIT ? OFFSET ?
"#;

const ADMIN_SUPPLIER_BY_REF_SQL: &str = r#"
    SELECT
        s.name AS ref,
        COALESCE(NULLIF(s.supplier_name, ''), s.name) AS name,
        COALESCE(s.mobile_no, '') AS phone
    FROM tabSupplier s
    WHERE s.disabled = 0
      AND s.name = ?
    LIMIT 1
"#;

const ADMIN_CUSTOMERS_PAGE_SQL: &str = r#"
    SELECT
        c.name AS ref,
        COALESCE(NULLIF(c.customer_name, ''), c.name) AS name,
        COALESCE(c.mobile_no, '') AS phone
    FROM tabCustomer c
    WHERE c.disabled = 0
      AND (? = '' OR c.name LIKE ? ESCAPE '\\' OR c.customer_name LIKE ? ESCAPE '\\' OR COALESCE(c.mobile_no, '') LIKE ? ESCAPE '\\')
    ORDER BY c.modified DESC
    LIMIT ? OFFSET ?
"#;

const ADMIN_CUSTOMER_BY_REF_SQL: &str = r#"
    SELECT
        c.name AS ref,
        COALESCE(NULLIF(c.customer_name, ''), c.name) AS name,
        COALESCE(c.mobile_no, '') AS phone
    FROM tabCustomer c
    WHERE c.disabled = 0
      AND c.name = ?
    LIMIT 1
"#;

const ADMIN_ITEMS_PAGE_SQL: &str = r#"
    SELECT
        i.name AS item_code,
        COALESCE(NULLIF(i.item_name, ''), i.name) AS item_name,
        COALESCE(i.stock_uom, '') AS stock_uom,
        COALESCE(i.item_group, '') AS item_group
    FROM tabItem i
    WHERE i.disabled = 0
      AND i.is_stock_item = 1
      AND (? = '' OR i.name LIKE ? ESCAPE '\\' OR i.item_name LIKE ? ESCAPE '\\')
    ORDER BY i.item_name ASC, i.name ASC
    LIMIT ? OFFSET ?
"#;

const ADMIN_ITEMS_BY_GROUP_PAGE_SQL: &str = r#"
    SELECT
        i.name AS item_code,
        COALESCE(NULLIF(i.item_name, ''), i.name) AS item_name,
        COALESCE(i.stock_uom, '') AS stock_uom,
        COALESCE(i.item_group, '') AS item_group
    FROM tabItem i
    WHERE i.disabled = 0
      AND i.is_stock_item = 1
      AND i.item_group = ?
      AND (? = '' OR i.name LIKE ? ESCAPE '\\' OR i.item_name LIKE ? ESCAPE '\\')
    ORDER BY i.item_name ASC, i.name ASC
    LIMIT ? OFFSET ?
"#;

const ADMIN_ITEM_GROUPS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(NULLIF(item_group_name, ''), name) AS item_group_name,
        COALESCE(parent_item_group, '') AS parent_item_group,
        COALESCE(is_group, 0) AS is_group
    FROM `tabItem Group`
    WHERE ? = '' OR name LIKE ? ESCAPE '\\' OR item_group_name LIKE ? ESCAPE '\\'
    ORDER BY name ASC
    LIMIT ?
"#;

const ADMIN_ITEM_GROUP_TREE_SQL: &str = r#"
    SELECT
        name,
        COALESCE(NULLIF(item_group_name, ''), name) AS item_group_name,
        COALESCE(parent_item_group, '') AS parent_item_group,
        COALESCE(is_group, 0) AS is_group
    FROM `tabItem Group`
    ORDER BY lft ASC, name ASC
"#;

const ADMIN_WAREHOUSES_SQL: &str = r#"
    SELECT
        name,
        COALESCE(company, '') AS company,
        COALESCE(is_group, 0) AS is_group,
        COALESCE(parent_warehouse, '') AS parent_warehouse
    FROM tabWarehouse
    WHERE COALESCE(disabled, 0) = 0
      AND (? = '' OR name LIKE ? ESCAPE '\\' OR company LIKE ? ESCAPE '\\')
      AND (? = '' OR parent_warehouse = ?)
    ORDER BY is_group ASC, name ASC
    LIMIT ?
"#;

const ADMIN_ASSIGNED_SUPPLIER_ITEMS_SQL: &str = r#"
    SELECT DISTINCT
        i.name AS item_code,
        COALESCE(NULLIF(i.item_name, ''), i.name) AS item_name,
        COALESCE(i.stock_uom, '') AS stock_uom,
        COALESCE(i.item_group, '') AS item_group
    FROM `tabItem Supplier` isup
    INNER JOIN tabItem i ON i.name = isup.parent
    WHERE isup.supplier = ?
      AND i.disabled = 0
      AND i.is_stock_item = 1
    ORDER BY i.item_name ASC, i.name ASC
    LIMIT ?
"#;

const ADMIN_CUSTOMER_ITEMS_SQL: &str = r#"
    SELECT DISTINCT
        i.name AS item_code,
        COALESCE(NULLIF(i.item_name, ''), i.name) AS item_name,
        COALESCE(i.stock_uom, '') AS stock_uom,
        COALESCE(i.item_group, '') AS item_group
    FROM `tabItem Customer Detail` icd
    INNER JOIN tabItem i ON i.name = icd.parent
    WHERE icd.customer_name = ?
      AND i.disabled = 0
      AND i.is_stock_item = 1
      AND (? = '' OR i.name LIKE ? ESCAPE '\\' OR i.item_name LIKE ? ESCAPE '\\')
    ORDER BY i.item_name ASC, i.name ASC
    LIMIT ?
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_admin_directory_row_without_changing_go_fields() {
        let entry = AdminDirectoryRow {
            ref_: "SUP-001".to_string(),
            name: "Best Supplier".to_string(),
            phone: "+99890".to_string(),
        }
        .into_entry();

        assert_eq!(entry.ref_, "SUP-001");
        assert_eq!(entry.name, "Best Supplier");
        assert_eq!(entry.phone, "+99890");
    }

    #[test]
    fn maps_admin_warehouse_row_from_erpnext_fields() {
        let warehouse = AdminWarehouseRow {
            name: " Stores - A ".to_string(),
            company: " Accord ".to_string(),
            is_group: 0,
            parent_warehouse: " Aparat ".to_string(),
        }
        .into_warehouse();

        assert_eq!(warehouse.warehouse, "Stores - A");
        assert_eq!(warehouse.company, "Accord");
        assert!(!warehouse.is_group);
        assert_eq!(warehouse.parent_warehouse, "Aparat");
    }

    #[test]
    fn maps_admin_item_row_with_default_warehouse() {
        let item = AdminItemRow {
            item_code: " ITEM-001 ".to_string(),
            item_name: " Milk ".to_string(),
            stock_uom: " Nos ".to_string(),
            item_group: " Drinks ".to_string(),
        }
        .into_item(" Stores - A ");

        assert_eq!(item.code, "ITEM-001");
        assert_eq!(item.name, "Milk");
        assert_eq!(item.uom, "Nos");
        assert_eq!(item.warehouse, "Stores - A");
        assert_eq!(item.item_group, "Drinks");
    }
}
