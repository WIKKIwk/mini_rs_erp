use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::core::admin::models::{AdminDirectoryEntry, AdminItemGroup};
use crate::core::profile::ports::{CustomerProfileRecord, SupplierProfileRecord};
use crate::core::werka::models::{
    CustomerDirectoryEntry, CustomerItemOption, SupplierDirectoryEntry, SupplierItem,
};
use crate::erpdb::catalog_cache::schema;
use crate::erpdb::werka_item_search::{
    SupplierItemSearchEntry, rank_customer_item_entries_by_query,
    rank_customer_item_options_by_query, rank_supplier_items_by_query, slice_page,
};
use crate::erpdb::werka_suppliers::clamp_limit;

mod directory_queries;
mod item_queries;
mod mapping_queries;
mod mutations;

#[derive(Debug, thiserror::Error)]
pub enum CatalogCacheError {
    #[error("catalog cache not ready")]
    NotReady,
    #[error("catalog cache lock failed")]
    LockFailed,
    #[error("catalog cache sqlite failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("catalog cache sync failed: {0}")]
    Sync(String),
    #[error("catalog cache io failed: {0}")]
    Io(#[from] std::io::Error),
}

pub struct CatalogCacheStore {
    conn: Mutex<Connection>,
    read_conns: Vec<Mutex<Connection>>,
    next_read_conn: AtomicUsize,
    ready: AtomicBool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedItem {
    pub name: String,
    pub item_name: String,
    pub stock_uom: String,
    pub item_group: String,
    pub modified: String,
    pub disabled: bool,
    pub is_stock_item: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedItemGroup {
    pub name: String,
    pub item_group_name: String,
    pub parent_item_group: String,
    pub is_group: bool,
    pub lft: i64,
    pub modified: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedSupplier {
    pub name: String,
    pub supplier_name: String,
    pub mobile_no: String,
    pub supplier_details: String,
    pub image: String,
    pub disabled: bool,
    pub modified: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedCustomer {
    pub name: String,
    pub customer_name: String,
    pub mobile_no: String,
    pub customer_details: String,
    pub disabled: bool,
    pub modified: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedItemSupplier {
    pub parent: String,
    pub supplier: String,
    pub modified: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedItemCustomer {
    pub parent: String,
    pub customer_name: String,
    pub modified: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogSnapshot {
    pub items: Vec<CachedItem>,
    pub item_groups: Vec<CachedItemGroup>,
    pub suppliers: Vec<CachedSupplier>,
    pub customers: Vec<CachedCustomer>,
    pub item_suppliers: Vec<CachedItemSupplier>,
    pub item_customers: Vec<CachedItemCustomer>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogKeySnapshot {
    pub items: Option<Vec<String>>,
    pub item_groups: Option<Vec<String>>,
    pub suppliers: Option<Vec<String>>,
    pub customers: Option<Vec<String>>,
    pub item_suppliers: Option<Vec<(String, String)>>,
    pub item_customers: Option<Vec<(String, String)>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogDeltaSnapshot {
    pub changed: CatalogSnapshot,
    pub keys: CatalogKeySnapshot,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogTableStats {
    pub count: i64,
    pub max_modified: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogStatsSnapshot {
    pub items: CatalogTableStats,
    pub item_groups: CatalogTableStats,
    pub suppliers: CatalogTableStats,
    pub customers: CatalogTableStats,
    pub item_suppliers: CatalogTableStats,
    pub item_customers: CatalogTableStats,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CatalogMissingChangedKeys {
    pub items: bool,
    pub item_groups: bool,
    pub suppliers: bool,
    pub customers: bool,
    pub item_suppliers: bool,
    pub item_customers: bool,
}

impl CatalogCacheStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CatalogCacheError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let conn = open_catalog_connection(path)?;
        schema::migrate(&conn)?;
        let read_conns = open_read_connections(path)?;
        Ok(Self {
            conn: Mutex::new(conn),
            read_conns,
            next_read_conn: AtomicUsize::new(0),
            ready: AtomicBool::new(false),
        })
    }

    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    fn lock_read(&self) -> Result<MutexGuard<'_, Connection>, CatalogCacheError> {
        let slot = if self.read_conns.is_empty() {
            &self.conn
        } else {
            let index = self.next_read_conn.fetch_add(1, Ordering::Relaxed) % self.read_conns.len();
            &self.read_conns[index]
        };
        slot.lock().map_err(|_| CatalogCacheError::LockFailed)
    }

    fn ensure_ready(&self) -> Result<(), CatalogCacheError> {
        if self.ready.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(CatalogCacheError::NotReady)
        }
    }
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>, CatalogCacheError> {
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(CatalogCacheError::from)
}

fn supplier_item_from_row(
    row: &rusqlite::Row<'_>,
    default_warehouse: &str,
) -> rusqlite::Result<SupplierItem> {
    let code: String = row.get(0)?;
    let name: String = row.get(1)?;
    Ok(SupplierItem {
        code: code.trim().to_string(),
        name: blank_default(&name, &code),
        uom: row.get::<_, String>(2)?.trim().to_string(),
        warehouse: default_warehouse.trim().to_string(),
        item_group: row.get::<_, String>(3)?.trim().to_string(),
    })
}

fn blank_default(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

fn bool_int(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn sqlite_like_pattern(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return "%".to_string();
    }
    let escaped = trimmed
        .replace('\\', r"\\")
        .replace('%', r"\%")
        .replace('_', r"\_");
    format!("%{escaped}%")
}

fn open_catalog_connection(path: &Path) -> Result<Connection, CatalogCacheError> {
    let conn = Connection::open(path)?;
    configure_catalog_connection(&conn)?;
    Ok(conn)
}

fn open_read_connections(path: &Path) -> Result<Vec<Mutex<Connection>>, CatalogCacheError> {
    let count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4)
        .clamp(4, 16);
    let mut connections = Vec::with_capacity(count);
    for _ in 0..count {
        connections.push(Mutex::new(open_catalog_connection(path)?));
    }
    Ok(connections)
}

fn configure_catalog_connection(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    register_catalog_collation(conn)?;
    Ok(())
}

fn register_catalog_collation(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_collation("ERP_CATALOG", |left, right| {
        catalog_sort_key(left).cmp(&catalog_sort_key(right))
    })
}

fn catalog_sort_key(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .replace(['’', '‘', 'ʻ', 'ʼ', '`'], "'")
        .replace('ﬁ', "fi")
        .replace('ﬀ', "ff")
        .replace('ﬂ', "fl")
        .replace('ﬃ', "ffi")
        .replace('ﬄ', "ffl")
}

fn profile_phone(mobile_no: &str, details: &str) -> String {
    if !mobile_no.trim().is_empty() {
        return mobile_no.trim().to_string();
    }
    for line in details.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("telefon:") {
            return trimmed["telefon:".len()..].trim().to_string();
        }
        if lower.starts_with("phone:") {
            return trimmed["phone:".len()..].trim().to_string();
        }
    }
    String::new()
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
