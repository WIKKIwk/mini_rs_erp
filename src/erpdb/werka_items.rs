use sqlx::{MySqlPool, query_as};

use crate::core::werka::models::{CustomerItemOption, SupplierItem};
use crate::erpdb::werka_item_search::{
    SupplierItemSearchEntry, append_search_terms, rank_customer_item_entries_by_query,
    rank_customer_item_options_by_query, rank_supplier_items_by_query, slice_page,
};
use crate::erpdb::werka_suppliers::clamp_limit;

pub(crate) async fn read_werka_customer_items(
    pool: &MySqlPool,
    default_warehouse: &str,
    customer_ref: &str,
    query: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<SupplierItem>, sqlx::Error> {
    let limit = clamp_limit(limit, 50, 500);
    if query.trim().is_empty() {
        let rows = query_as::<_, SupplierItemRow>(CUSTOMER_ITEMS_SQL)
            .bind(customer_ref.trim())
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(pool)
            .await?;
        return Ok(rows
            .into_iter()
            .map(|row| row.into_item(default_warehouse))
            .collect());
    }

    let entries = load_all_customer_item_entries(pool, default_warehouse, customer_ref).await?;
    Ok(slice_page(
        &rank_customer_item_entries_by_query(entries, query),
        offset,
        limit,
    ))
}

pub(crate) async fn read_werka_supplier_items(
    pool: &MySqlPool,
    default_warehouse: &str,
    supplier_ref: &str,
    query: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<SupplierItem>, sqlx::Error> {
    let limit = clamp_limit(limit, 50, 500);
    if query.trim().is_empty() {
        let rows = query_as::<_, SupplierItemRow>(SUPPLIER_ITEMS_PAGE_SQL)
            .bind(supplier_ref.trim())
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(pool)
            .await?;
        return Ok(rows
            .into_iter()
            .map(|row| row.into_item(default_warehouse))
            .collect());
    }

    let items = load_all_supplier_items(pool, default_warehouse, supplier_ref).await?;
    Ok(slice_page(
        &rank_supplier_items_by_query(items, query),
        offset,
        limit,
    ))
}

pub(crate) async fn read_werka_customer_item_options(
    pool: &MySqlPool,
    default_warehouse: &str,
    query: &str,
    limit: usize,
    offset: usize,
) -> Result<Vec<CustomerItemOption>, sqlx::Error> {
    let limit = clamp_limit(limit, 50, 500);
    if query.trim().is_empty() {
        let rows = query_as::<_, CustomerItemOptionRow>(CUSTOMER_ITEM_OPTIONS_PAGE_SQL)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(pool)
            .await?;
        return Ok(rows
            .into_iter()
            .map(|row| row.into_option(default_warehouse))
            .collect());
    }

    let items = load_all_customer_item_options(pool, default_warehouse).await?;
    Ok(slice_page(
        &rank_customer_item_options_by_query(items, query),
        offset,
        limit,
    ))
}

async fn load_all_customer_item_entries(
    pool: &MySqlPool,
    default_warehouse: &str,
    customer_ref: &str,
) -> Result<Vec<SupplierItemSearchEntry>, sqlx::Error> {
    let rows = query_as::<_, CustomerItemSearchRow>(CUSTOMER_ITEMS_SEARCH_SQL)
        .bind(customer_ref.trim())
        .fetch_all(pool)
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let mut terms = vec![row.item_code.clone(), row.item_name.clone()];
            append_search_terms(&mut terms, &row.customer_refs);
            append_search_terms(&mut terms, &row.customer_names);
            SupplierItemSearchEntry {
                item: SupplierItem {
                    code: row.item_code,
                    name: row.item_name,
                    uom: row.stock_uom,
                    warehouse: default_warehouse.trim().to_string(),
                    item_group: String::new(),
                },
                search_terms: terms,
            }
        })
        .collect())
}

async fn load_all_supplier_items(
    pool: &MySqlPool,
    default_warehouse: &str,
    supplier_ref: &str,
) -> Result<Vec<SupplierItem>, sqlx::Error> {
    let rows = query_as::<_, SupplierItemRow>(SUPPLIER_ITEMS_ALL_SQL)
        .bind(supplier_ref.trim())
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| row.into_item(default_warehouse))
        .collect())
}

async fn load_all_customer_item_options(
    pool: &MySqlPool,
    default_warehouse: &str,
) -> Result<Vec<CustomerItemOption>, sqlx::Error> {
    let rows = query_as::<_, CustomerItemOptionRow>(CUSTOMER_ITEM_OPTIONS_ALL_SQL)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| row.into_option(default_warehouse))
        .collect())
}

#[derive(Debug, sqlx::FromRow)]
struct SupplierItemRow {
    item_code: String,
    item_name: String,
    stock_uom: String,
}

impl SupplierItemRow {
    fn into_item(self, default_warehouse: &str) -> SupplierItem {
        SupplierItem {
            code: self.item_code,
            name: self.item_name,
            uom: self.stock_uom,
            warehouse: default_warehouse.trim().to_string(),
            item_group: String::new(),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct CustomerItemSearchRow {
    item_code: String,
    item_name: String,
    stock_uom: String,
    customer_refs: String,
    customer_names: String,
}

#[derive(Debug, sqlx::FromRow)]
struct CustomerItemOptionRow {
    customer_ref: String,
    customer_name: String,
    customer_phone: String,
    item_code: String,
    item_name: String,
    stock_uom: String,
}

impl CustomerItemOptionRow {
    fn into_option(self, default_warehouse: &str) -> CustomerItemOption {
        CustomerItemOption {
            customer_ref: self.customer_ref,
            customer_name: self.customer_name,
            customer_phone: self.customer_phone,
            item_code: self.item_code,
            item_name: self.item_name,
            uom: self.stock_uom,
            warehouse: default_warehouse.trim().to_string(),
        }
    }
}

const CUSTOMER_ITEMS_SQL: &str = r#"
    SELECT DISTINCT
        i.item_code,
        COALESCE(NULLIF(i.item_name, ''), i.item_code) AS item_name,
        COALESCE(NULLIF(i.stock_uom, ''), 'Nos') AS stock_uom
    FROM `tabItem Customer Detail` icd
    INNER JOIN tabItem i ON i.name = icd.parent
    WHERE icd.customer_name = ?
      AND i.disabled = 0
    ORDER BY i.item_name ASC
    LIMIT ? OFFSET ?
"#;

const CUSTOMER_ITEMS_SEARCH_SQL: &str = r#"
    SELECT
        i.item_code,
        COALESCE(NULLIF(i.item_name, ''), i.item_code) AS item_name,
        COALESCE(NULLIF(i.stock_uom, ''), 'Nos') AS stock_uom,
        COALESCE(GROUP_CONCAT(DISTINCT icd_all.customer_name ORDER BY icd_all.customer_name SEPARATOR '\n'), '') AS customer_refs,
        COALESCE(GROUP_CONCAT(DISTINCT COALESCE(NULLIF(c.customer_name, ''), c.name) ORDER BY COALESCE(NULLIF(c.customer_name, ''), c.name) SEPARATOR '\n'), '') AS customer_names
    FROM `tabItem Customer Detail` icd_selected
    INNER JOIN tabItem i ON i.name = icd_selected.parent
    LEFT JOIN `tabItem Customer Detail` icd_all ON icd_all.parent = i.name
    LEFT JOIN tabCustomer c ON c.name = icd_all.customer_name
    WHERE icd_selected.customer_name = ?
      AND i.disabled = 0
    GROUP BY i.item_code, item_name, stock_uom
    ORDER BY item_name ASC
"#;

const SUPPLIER_ITEMS_PAGE_SQL: &str = r#"
    SELECT DISTINCT
        i.item_code,
        COALESCE(NULLIF(i.item_name, ''), i.item_code) AS item_name,
        COALESCE(NULLIF(i.stock_uom, ''), 'Nos') AS stock_uom
    FROM `tabItem Supplier` isup
    INNER JOIN tabItem i ON i.name = isup.parent
    WHERE isup.supplier = ?
      AND i.disabled = 0
    ORDER BY i.item_name ASC
    LIMIT ? OFFSET ?
"#;

const SUPPLIER_ITEMS_ALL_SQL: &str = r#"
    SELECT DISTINCT
        i.item_code,
        COALESCE(NULLIF(i.item_name, ''), i.item_code) AS item_name,
        COALESCE(NULLIF(i.stock_uom, ''), 'Nos') AS stock_uom
    FROM `tabItem Supplier` isup
    INNER JOIN tabItem i ON i.name = isup.parent
    WHERE isup.supplier = ?
      AND i.disabled = 0
    ORDER BY i.item_name ASC
"#;

const CUSTOMER_ITEM_OPTIONS_PAGE_SQL: &str = r#"
    SELECT DISTINCT
        c.name AS customer_ref,
        COALESCE(NULLIF(c.customer_name, ''), c.name) AS customer_name,
        COALESCE(c.mobile_no, '') AS customer_phone,
        i.item_code,
        COALESCE(NULLIF(i.item_name, ''), i.item_code) AS item_name,
        COALESCE(NULLIF(i.stock_uom, ''), 'Nos') AS stock_uom
    FROM `tabItem Customer Detail` icd
    INNER JOIN tabItem i ON i.name = icd.parent
    INNER JOIN tabCustomer c ON c.name = icd.customer_name
    WHERE c.disabled = 0
      AND i.disabled = 0
    ORDER BY i.item_name ASC, c.customer_name ASC
    LIMIT ? OFFSET ?
"#;

const CUSTOMER_ITEM_OPTIONS_ALL_SQL: &str = r#"
    SELECT DISTINCT
        c.name AS customer_ref,
        COALESCE(NULLIF(c.customer_name, ''), c.name) AS customer_name,
        COALESCE(c.mobile_no, '') AS customer_phone,
        i.item_code,
        COALESCE(NULLIF(i.item_name, ''), i.item_code) AS item_name,
        COALESCE(NULLIF(i.stock_uom, ''), 'Nos') AS stock_uom
    FROM `tabItem Customer Detail` icd
    INNER JOIN tabItem i ON i.name = icd.parent
    INNER JOIN tabCustomer c ON c.name = icd.customer_name
    WHERE c.disabled = 0
      AND i.disabled = 0
    ORDER BY i.item_name ASC, c.customer_name ASC
"#;
