use rusqlite::Connection;

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS catalog_items (
            name TEXT PRIMARY KEY NOT NULL,
            item_name TEXT NOT NULL DEFAULT '',
            stock_uom TEXT NOT NULL DEFAULT '',
            item_group TEXT NOT NULL DEFAULT '',
            modified TEXT NOT NULL DEFAULT '',
            disabled INTEGER NOT NULL DEFAULT 0,
            is_stock_item INTEGER NOT NULL DEFAULT 1
        );

        CREATE INDEX IF NOT EXISTS idx_catalog_items_name
            ON catalog_items(name);
        CREATE INDEX IF NOT EXISTS idx_catalog_items_item_name
            ON catalog_items(item_name);
        CREATE INDEX IF NOT EXISTS idx_catalog_items_group
            ON catalog_items(item_group);
        CREATE INDEX IF NOT EXISTS idx_catalog_items_active_sort
            ON catalog_items(disabled, is_stock_item, item_name COLLATE ERP_CATALOG, name COLLATE ERP_CATALOG);
        CREATE INDEX IF NOT EXISTS idx_catalog_items_group_active_sort
            ON catalog_items(item_group, disabled, is_stock_item, item_name COLLATE ERP_CATALOG, name COLLATE ERP_CATALOG);

        CREATE TABLE IF NOT EXISTS catalog_item_groups (
            name TEXT PRIMARY KEY NOT NULL,
            item_group_name TEXT NOT NULL DEFAULT '',
            parent_item_group TEXT NOT NULL DEFAULT '',
            is_group INTEGER NOT NULL DEFAULT 0,
            lft INTEGER NOT NULL DEFAULT 0,
            modified TEXT NOT NULL DEFAULT ''
        );

        CREATE INDEX IF NOT EXISTS idx_catalog_item_groups_lft
            ON catalog_item_groups(lft);
        CREATE INDEX IF NOT EXISTS idx_catalog_item_groups_parent
            ON catalog_item_groups(parent_item_group);

        CREATE TABLE IF NOT EXISTS catalog_suppliers (
            name TEXT PRIMARY KEY NOT NULL,
            supplier_name TEXT NOT NULL DEFAULT '',
            mobile_no TEXT NOT NULL DEFAULT '',
            supplier_details TEXT NOT NULL DEFAULT '',
            image TEXT NOT NULL DEFAULT '',
            disabled INTEGER NOT NULL DEFAULT 0,
            modified TEXT NOT NULL DEFAULT ''
        );

        CREATE INDEX IF NOT EXISTS idx_catalog_suppliers_name
            ON catalog_suppliers(name);
        CREATE INDEX IF NOT EXISTS idx_catalog_suppliers_supplier_name
            ON catalog_suppliers(supplier_name);

        CREATE TABLE IF NOT EXISTS catalog_customers (
            name TEXT PRIMARY KEY NOT NULL,
            customer_name TEXT NOT NULL DEFAULT '',
            mobile_no TEXT NOT NULL DEFAULT '',
            customer_details TEXT NOT NULL DEFAULT '',
            disabled INTEGER NOT NULL DEFAULT 0,
            modified TEXT NOT NULL DEFAULT ''
        );

        CREATE INDEX IF NOT EXISTS idx_catalog_customers_name
            ON catalog_customers(name);
        CREATE INDEX IF NOT EXISTS idx_catalog_customers_customer_name
            ON catalog_customers(customer_name);

        CREATE TABLE IF NOT EXISTS catalog_item_suppliers (
            parent TEXT NOT NULL,
            supplier TEXT NOT NULL,
            modified TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (parent, supplier)
        );

        CREATE INDEX IF NOT EXISTS idx_catalog_item_suppliers_supplier
            ON catalog_item_suppliers(supplier);
        CREATE INDEX IF NOT EXISTS idx_catalog_item_suppliers_parent
            ON catalog_item_suppliers(parent);

        CREATE TABLE IF NOT EXISTS catalog_item_customers (
            parent TEXT NOT NULL,
            customer_name TEXT NOT NULL,
            modified TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (parent, customer_name)
        );

        CREATE INDEX IF NOT EXISTS idx_catalog_item_customers_customer
            ON catalog_item_customers(customer_name);
        CREATE INDEX IF NOT EXISTS idx_catalog_item_customers_parent
            ON catalog_item_customers(parent);

        CREATE TABLE IF NOT EXISTS catalog_sync_state (
            scope TEXT PRIMARY KEY NOT NULL,
            last_full_sync_at TEXT NOT NULL DEFAULT '',
            last_delta_sync_at TEXT NOT NULL DEFAULT '',
            last_modified TEXT NOT NULL DEFAULT ''
        );
        "#,
    )
}

#[cfg(test)]
mod tests {
    use super::migrate;
    use rusqlite::Connection;

    #[test]
    fn migrate_creates_catalog_tables_and_indexes() {
        let conn = Connection::open_in_memory().expect("open sqlite");
        conn.create_collation("ERP_CATALOG", |left, right| {
            left.trim().to_lowercase().cmp(&right.trim().to_lowercase())
        })
        .expect("catalog collation");

        migrate(&conn).expect("migrate");
        migrate(&conn).expect("migrate is idempotent");

        for table in [
            "catalog_items",
            "catalog_item_groups",
            "catalog_suppliers",
            "catalog_customers",
            "catalog_item_suppliers",
            "catalog_item_customers",
            "catalog_sync_state",
        ] {
            assert!(
                object_exists(&conn, "table", table),
                "missing table {table}"
            );
        }

        for index in [
            "idx_catalog_items_name",
            "idx_catalog_items_item_name",
            "idx_catalog_items_group",
            "idx_catalog_items_active_sort",
            "idx_catalog_items_group_active_sort",
            "idx_catalog_item_groups_lft",
            "idx_catalog_item_suppliers_supplier",
            "idx_catalog_item_customers_customer",
        ] {
            assert!(
                object_exists(&conn, "index", index),
                "missing index {index}"
            );
        }
    }

    fn object_exists(conn: &Connection, kind: &str, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2 LIMIT 1",
            (kind, name),
            |_| Ok(()),
        )
        .is_ok()
    }
}
