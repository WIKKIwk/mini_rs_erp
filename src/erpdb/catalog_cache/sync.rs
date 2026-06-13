use sqlx::query_as;

use crate::erpdb::catalog_cache::store::{
    CachedCustomer, CachedItem, CachedItemCustomer, CachedItemGroup, CachedItemSupplier,
    CachedSupplier, CatalogCacheError, CatalogCacheStore, CatalogDeltaSnapshot, CatalogKeySnapshot,
    CatalogMissingChangedKeys, CatalogSnapshot, CatalogStatsSnapshot, CatalogTableStats,
};
use crate::erpdb::reader::DirectDbReader;

#[path = "sync_sql.rs"]
mod sql;
use sql::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogSyncReport {
    pub items: usize,
    pub item_groups: usize,
    pub suppliers: usize,
    pub customers: usize,
    pub item_suppliers: usize,
    pub item_customers: usize,
}

pub async fn sync_catalog_once(
    direct: &DirectDbReader,
    store: &CatalogCacheStore,
) -> Result<CatalogSyncReport, CatalogCacheError> {
    let items = query_as::<_, ItemRow>(ITEMS_SQL)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let item_groups = query_as::<_, ItemGroupRow>(ITEM_GROUPS_SQL)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let suppliers = query_as::<_, SupplierRow>(SUPPLIERS_SQL)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let customers = query_as::<_, CustomerRow>(CUSTOMERS_SQL)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let item_suppliers = query_as::<_, ItemSupplierRow>(ITEM_SUPPLIERS_SQL)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let item_customers = query_as::<_, ItemCustomerRow>(ITEM_CUSTOMERS_SQL)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;

    store.replace_catalog(CatalogSnapshot {
        items: items.iter().map(ItemRow::to_cached).collect(),
        item_groups: item_groups.iter().map(ItemGroupRow::to_cached).collect(),
        suppliers: suppliers.iter().map(SupplierRow::to_cached).collect(),
        customers: customers.iter().map(CustomerRow::to_cached).collect(),
        item_suppliers: item_suppliers
            .iter()
            .map(ItemSupplierRow::to_cached)
            .collect(),
        item_customers: item_customers
            .iter()
            .map(ItemCustomerRow::to_cached)
            .collect(),
    })?;

    Ok(CatalogSyncReport {
        items: items.len(),
        item_groups: item_groups.len(),
        suppliers: suppliers.len(),
        customers: customers.len(),
        item_suppliers: item_suppliers.len(),
        item_customers: item_customers.len(),
    })
}

pub async fn sync_catalog_delta_once(
    direct: &DirectDbReader,
    store: &CatalogCacheStore,
) -> Result<CatalogSyncReport, CatalogCacheError> {
    let local_stats = store.stats()?;
    let remote_stats = fetch_catalog_stats(direct).await?;
    let items = query_as::<_, ItemRow>(CHANGED_ITEMS_SQL)
        .bind(&local_stats.items.max_modified)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let item_groups = query_as::<_, ItemGroupRow>(CHANGED_ITEM_GROUPS_SQL)
        .bind(&local_stats.item_groups.max_modified)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let suppliers = query_as::<_, SupplierRow>(CHANGED_SUPPLIERS_SQL)
        .bind(&local_stats.suppliers.max_modified)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let customers = query_as::<_, CustomerRow>(CHANGED_CUSTOMERS_SQL)
        .bind(&local_stats.customers.max_modified)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let item_suppliers = query_as::<_, ItemSupplierRow>(CHANGED_ITEM_SUPPLIERS_SQL)
        .bind(&local_stats.item_suppliers.max_modified)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let item_customers = query_as::<_, ItemCustomerRow>(CHANGED_ITEM_CUSTOMERS_SQL)
        .bind(&local_stats.item_customers.max_modified)
        .fetch_all(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    let changed = CatalogSnapshot {
        items: items.iter().map(ItemRow::to_cached).collect(),
        item_groups: item_groups.iter().map(ItemGroupRow::to_cached).collect(),
        suppliers: suppliers.iter().map(SupplierRow::to_cached).collect(),
        customers: customers.iter().map(CustomerRow::to_cached).collect(),
        item_suppliers: item_suppliers
            .iter()
            .map(ItemSupplierRow::to_cached)
            .collect(),
        item_customers: item_customers
            .iter()
            .map(ItemCustomerRow::to_cached)
            .collect(),
    };
    let missing_changed_keys = store.missing_changed_keys(&changed)?;
    let keys =
        fetch_catalog_keys(direct, &local_stats, &remote_stats, missing_changed_keys).await?;

    store.apply_delta(CatalogDeltaSnapshot { changed, keys })?;

    Ok(CatalogSyncReport {
        items: items.len(),
        item_groups: item_groups.len(),
        suppliers: suppliers.len(),
        customers: customers.len(),
        item_suppliers: item_suppliers.len(),
        item_customers: item_customers.len(),
    })
}

async fn fetch_catalog_stats(
    direct: &DirectDbReader,
) -> Result<CatalogStatsSnapshot, CatalogCacheError> {
    Ok(CatalogStatsSnapshot {
        items: fetch_table_stats(direct, ITEM_STATS_SQL).await?,
        item_groups: fetch_table_stats(direct, ITEM_GROUP_STATS_SQL).await?,
        suppliers: fetch_table_stats(direct, SUPPLIER_STATS_SQL).await?,
        customers: fetch_table_stats(direct, CUSTOMER_STATS_SQL).await?,
        item_suppliers: fetch_table_stats(direct, ITEM_SUPPLIER_STATS_SQL).await?,
        item_customers: fetch_table_stats(direct, ITEM_CUSTOMER_STATS_SQL).await?,
    })
}

async fn fetch_table_stats(
    direct: &DirectDbReader,
    sql: &str,
) -> Result<CatalogTableStats, CatalogCacheError> {
    let row = query_as::<_, TableStatsRow>(sql)
        .fetch_one(&direct.pool)
        .await
        .map_err(map_sqlx)?;
    Ok(CatalogTableStats {
        count: row.row_count,
        max_modified: row.max_modified.trim().to_string(),
    })
}

async fn fetch_catalog_keys(
    direct: &DirectDbReader,
    local_stats: &CatalogStatsSnapshot,
    remote_stats: &CatalogStatsSnapshot,
    missing_changed_keys: CatalogMissingChangedKeys,
) -> Result<CatalogKeySnapshot, CatalogCacheError> {
    Ok(CatalogKeySnapshot {
        items: fetch_single_keys_if_count_changed(
            direct,
            ITEM_KEYS_SQL,
            local_stats.items.count,
            remote_stats.items.count,
            missing_changed_keys.items,
        )
        .await?,
        item_groups: fetch_single_keys_if_count_changed(
            direct,
            ITEM_GROUP_KEYS_SQL,
            local_stats.item_groups.count,
            remote_stats.item_groups.count,
            missing_changed_keys.item_groups,
        )
        .await?,
        suppliers: fetch_single_keys_if_count_changed(
            direct,
            SUPPLIER_KEYS_SQL,
            local_stats.suppliers.count,
            remote_stats.suppliers.count,
            missing_changed_keys.suppliers,
        )
        .await?,
        customers: fetch_single_keys_if_count_changed(
            direct,
            CUSTOMER_KEYS_SQL,
            local_stats.customers.count,
            remote_stats.customers.count,
            missing_changed_keys.customers,
        )
        .await?,
        item_suppliers: fetch_composite_keys_if_count_changed(
            direct,
            ITEM_SUPPLIER_KEYS_SQL,
            local_stats.item_suppliers.count,
            remote_stats.item_suppliers.count,
            missing_changed_keys.item_suppliers,
        )
        .await?,
        item_customers: fetch_composite_keys_if_count_changed(
            direct,
            ITEM_CUSTOMER_KEYS_SQL,
            local_stats.item_customers.count,
            remote_stats.item_customers.count,
            missing_changed_keys.item_customers,
        )
        .await?,
    })
}

async fn fetch_single_keys_if_count_changed(
    direct: &DirectDbReader,
    sql: &str,
    local_count: i64,
    remote_count: i64,
    force: bool,
) -> Result<Option<Vec<String>>, CatalogCacheError> {
    if local_count == remote_count && !force {
        return Ok(None);
    }
    Ok(Some(
        sqlx::query_scalar::<_, String>(sql)
            .fetch_all(&direct.pool)
            .await
            .map_err(map_sqlx)?
            .into_iter()
            .map(|value| value.trim().to_string())
            .collect(),
    ))
}

async fn fetch_composite_keys_if_count_changed(
    direct: &DirectDbReader,
    sql: &str,
    local_count: i64,
    remote_count: i64,
    force: bool,
) -> Result<Option<Vec<(String, String)>>, CatalogCacheError> {
    if local_count == remote_count && !force {
        return Ok(None);
    }
    Ok(Some(
        query_as::<_, CompositeKeyRow>(sql)
            .fetch_all(&direct.pool)
            .await
            .map_err(map_sqlx)?
            .into_iter()
            .map(|row| {
                (
                    row.left_key.trim().to_string(),
                    row.right_key.trim().to_string(),
                )
            })
            .collect(),
    ))
}

#[derive(Debug, sqlx::FromRow)]
struct ItemRow {
    name: String,
    item_name: String,
    stock_uom: String,
    item_group: String,
    modified: String,
    disabled: i32,
    is_stock_item: i32,
}

impl ItemRow {
    fn to_cached(&self) -> CachedItem {
        CachedItem {
            name: self.name.trim().to_string(),
            item_name: self.item_name.trim().to_string(),
            stock_uom: self.stock_uom.trim().to_string(),
            item_group: self.item_group.trim().to_string(),
            modified: self.modified.trim().to_string(),
            disabled: self.disabled != 0,
            is_stock_item: self.is_stock_item != 0,
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ItemGroupRow {
    name: String,
    item_group_name: String,
    parent_item_group: String,
    is_group: i32,
    lft: i64,
    modified: String,
}

impl ItemGroupRow {
    fn to_cached(&self) -> CachedItemGroup {
        CachedItemGroup {
            name: self.name.trim().to_string(),
            item_group_name: self.item_group_name.trim().to_string(),
            parent_item_group: self.parent_item_group.trim().to_string(),
            is_group: self.is_group != 0,
            lft: self.lft,
            modified: self.modified.trim().to_string(),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct SupplierRow {
    name: String,
    supplier_name: String,
    mobile_no: String,
    supplier_details: String,
    image: String,
    disabled: i32,
    modified: String,
}

impl SupplierRow {
    fn to_cached(&self) -> CachedSupplier {
        CachedSupplier {
            name: self.name.trim().to_string(),
            supplier_name: self.supplier_name.trim().to_string(),
            mobile_no: self.mobile_no.trim().to_string(),
            supplier_details: self.supplier_details.trim().to_string(),
            image: self.image.trim().to_string(),
            disabled: self.disabled != 0,
            modified: self.modified.trim().to_string(),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct CustomerRow {
    name: String,
    customer_name: String,
    mobile_no: String,
    customer_details: String,
    disabled: i32,
    modified: String,
}

impl CustomerRow {
    fn to_cached(&self) -> CachedCustomer {
        CachedCustomer {
            name: self.name.trim().to_string(),
            customer_name: self.customer_name.trim().to_string(),
            mobile_no: self.mobile_no.trim().to_string(),
            customer_details: self.customer_details.trim().to_string(),
            disabled: self.disabled != 0,
            modified: self.modified.trim().to_string(),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ItemSupplierRow {
    parent: String,
    supplier: String,
    modified: String,
}

impl ItemSupplierRow {
    fn to_cached(&self) -> CachedItemSupplier {
        CachedItemSupplier {
            parent: self.parent.trim().to_string(),
            supplier: self.supplier.trim().to_string(),
            modified: self.modified.trim().to_string(),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ItemCustomerRow {
    parent: String,
    customer_name: String,
    modified: String,
}

#[derive(Debug, sqlx::FromRow)]
struct CompositeKeyRow {
    left_key: String,
    right_key: String,
}

#[derive(Debug, sqlx::FromRow)]
struct TableStatsRow {
    row_count: i64,
    max_modified: String,
}

impl ItemCustomerRow {
    fn to_cached(&self) -> CachedItemCustomer {
        CachedItemCustomer {
            parent: self.parent.trim().to_string(),
            customer_name: self.customer_name.trim().to_string(),
            modified: self.modified.trim().to_string(),
        }
    }
}

fn map_sqlx(error: sqlx::Error) -> CatalogCacheError {
    CatalogCacheError::Sync(error.to_string())
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
