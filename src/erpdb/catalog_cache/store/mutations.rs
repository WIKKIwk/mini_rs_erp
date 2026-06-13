use super::*;

impl CatalogCacheStore {
    pub fn replace_catalog(&self, snapshot: CatalogSnapshot) -> Result<(), CatalogCacheError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let tx = conn.transaction()?;
        tx.execute_batch(
            r#"
            DELETE FROM catalog_item_suppliers;
            DELETE FROM catalog_item_customers;
            DELETE FROM catalog_items;
            DELETE FROM catalog_item_groups;
            DELETE FROM catalog_suppliers;
            DELETE FROM catalog_customers;
            "#,
        )?;
        insert_items(&tx, &snapshot.items)?;
        insert_item_groups(&tx, &snapshot.item_groups)?;
        insert_suppliers(&tx, &snapshot.suppliers)?;
        insert_customers(&tx, &snapshot.customers)?;
        insert_item_suppliers(&tx, &snapshot.item_suppliers)?;
        insert_item_customers(&tx, &snapshot.item_customers)?;
        tx.commit()?;
        self.mark_ready();
        Ok(())
    }

    pub fn apply_delta(&self, delta: CatalogDeltaSnapshot) -> Result<(), CatalogCacheError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| CatalogCacheError::LockFailed)?;
        let tx = conn.transaction()?;
        insert_items(&tx, &delta.changed.items)?;
        insert_item_groups(&tx, &delta.changed.item_groups)?;
        insert_suppliers(&tx, &delta.changed.suppliers)?;
        insert_customers(&tx, &delta.changed.customers)?;
        insert_item_suppliers(&tx, &delta.changed.item_suppliers)?;
        insert_item_customers(&tx, &delta.changed.item_customers)?;
        if let Some(keys) = &delta.keys.items {
            retain_single_key_table(&tx, "catalog_items", "name", keys)?;
        }
        if let Some(keys) = &delta.keys.item_groups {
            retain_single_key_table(&tx, "catalog_item_groups", "name", keys)?;
        }
        if let Some(keys) = &delta.keys.suppliers {
            retain_single_key_table(&tx, "catalog_suppliers", "name", keys)?;
        }
        if let Some(keys) = &delta.keys.customers {
            retain_single_key_table(&tx, "catalog_customers", "name", keys)?;
        }
        if let Some(keys) = &delta.keys.item_suppliers {
            retain_composite_key_table(
                &tx,
                "catalog_item_suppliers",
                "parent",
                "supplier",
                "temp_catalog_item_supplier_keys",
                keys,
            )?;
        }
        if let Some(keys) = &delta.keys.item_customers {
            retain_composite_key_table(
                &tx,
                "catalog_item_customers",
                "parent",
                "customer_name",
                "temp_catalog_item_customer_keys",
                keys,
            )?;
        }
        tx.commit()?;
        self.mark_ready();
        Ok(())
    }

    pub fn stats(&self) -> Result<CatalogStatsSnapshot, CatalogCacheError> {
        let conn = self.lock_read()?;
        Ok(CatalogStatsSnapshot {
            items: table_stats(&conn, "catalog_items")?,
            item_groups: table_stats(&conn, "catalog_item_groups")?,
            suppliers: table_stats(&conn, "catalog_suppliers")?,
            customers: table_stats(&conn, "catalog_customers")?,
            item_suppliers: table_stats(&conn, "catalog_item_suppliers")?,
            item_customers: table_stats(&conn, "catalog_item_customers")?,
        })
    }

    pub fn missing_changed_keys(
        &self,
        changed: &CatalogSnapshot,
    ) -> Result<CatalogMissingChangedKeys, CatalogCacheError> {
        let conn = self.lock_read()?;
        Ok(CatalogMissingChangedKeys {
            items: single_keys_missing(
                &conn,
                "catalog_items",
                "name",
                changed.items.iter().map(|row| row.name.as_str()),
            )?,
            item_groups: single_keys_missing(
                &conn,
                "catalog_item_groups",
                "name",
                changed.item_groups.iter().map(|row| row.name.as_str()),
            )?,
            suppliers: single_keys_missing(
                &conn,
                "catalog_suppliers",
                "name",
                changed.suppliers.iter().map(|row| row.name.as_str()),
            )?,
            customers: single_keys_missing(
                &conn,
                "catalog_customers",
                "name",
                changed.customers.iter().map(|row| row.name.as_str()),
            )?,
            item_suppliers: composite_keys_missing(
                &conn,
                "catalog_item_suppliers",
                "parent",
                "supplier",
                changed
                    .item_suppliers
                    .iter()
                    .map(|row| (row.parent.as_str(), row.supplier.as_str())),
            )?,
            item_customers: composite_keys_missing(
                &conn,
                "catalog_item_customers",
                "parent",
                "customer_name",
                changed
                    .item_customers
                    .iter()
                    .map(|row| (row.parent.as_str(), row.customer_name.as_str())),
            )?,
        })
    }
}
fn insert_items(tx: &rusqlite::Transaction<'_>, items: &[CachedItem]) -> rusqlite::Result<()> {
    for item in items {
        tx.execute(
            r#"
            INSERT INTO catalog_items
                (name, item_name, stock_uom, item_group, modified, disabled, is_stock_item)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(name) DO UPDATE SET
                item_name = excluded.item_name,
                stock_uom = excluded.stock_uom,
                item_group = excluded.item_group,
                modified = excluded.modified,
                disabled = excluded.disabled,
                is_stock_item = excluded.is_stock_item
            "#,
            params![
                item.name.trim(),
                blank_default(&item.item_name, &item.name),
                item.stock_uom.trim(),
                item.item_group.trim(),
                item.modified.trim(),
                bool_int(item.disabled),
                bool_int(item.is_stock_item),
            ],
        )?;
    }
    Ok(())
}

fn insert_item_groups(
    tx: &rusqlite::Transaction<'_>,
    groups: &[CachedItemGroup],
) -> rusqlite::Result<()> {
    for group in groups {
        tx.execute(
            r#"
            INSERT INTO catalog_item_groups
                (name, item_group_name, parent_item_group, is_group, lft, modified)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(name) DO UPDATE SET
                item_group_name = excluded.item_group_name,
                parent_item_group = excluded.parent_item_group,
                is_group = excluded.is_group,
                lft = excluded.lft,
                modified = excluded.modified
            "#,
            params![
                group.name.trim(),
                blank_default(&group.item_group_name, &group.name),
                group.parent_item_group.trim(),
                bool_int(group.is_group),
                group.lft,
                group.modified.trim(),
            ],
        )?;
    }
    Ok(())
}

fn insert_suppliers(
    tx: &rusqlite::Transaction<'_>,
    suppliers: &[CachedSupplier],
) -> rusqlite::Result<()> {
    for supplier in suppliers {
        tx.execute(
            r#"
            INSERT INTO catalog_suppliers
                (name, supplier_name, mobile_no, supplier_details, image, disabled, modified)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(name) DO UPDATE SET
                supplier_name = excluded.supplier_name,
                mobile_no = excluded.mobile_no,
                supplier_details = excluded.supplier_details,
                image = excluded.image,
                disabled = excluded.disabled,
                modified = excluded.modified
            "#,
            params![
                supplier.name.trim(),
                blank_default(&supplier.supplier_name, &supplier.name),
                supplier.mobile_no.trim(),
                supplier.supplier_details.trim(),
                supplier.image.trim(),
                bool_int(supplier.disabled),
                supplier.modified.trim(),
            ],
        )?;
    }
    Ok(())
}

fn insert_customers(
    tx: &rusqlite::Transaction<'_>,
    customers: &[CachedCustomer],
) -> rusqlite::Result<()> {
    for customer in customers {
        tx.execute(
            r#"
            INSERT INTO catalog_customers
                (name, customer_name, mobile_no, customer_details, disabled, modified)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(name) DO UPDATE SET
                customer_name = excluded.customer_name,
                mobile_no = excluded.mobile_no,
                customer_details = excluded.customer_details,
                disabled = excluded.disabled,
                modified = excluded.modified
            "#,
            params![
                customer.name.trim(),
                blank_default(&customer.customer_name, &customer.name),
                customer.mobile_no.trim(),
                customer.customer_details.trim(),
                bool_int(customer.disabled),
                customer.modified.trim(),
            ],
        )?;
    }
    Ok(())
}

fn insert_item_suppliers(
    tx: &rusqlite::Transaction<'_>,
    links: &[CachedItemSupplier],
) -> rusqlite::Result<()> {
    for link in links {
        tx.execute(
            r#"
            INSERT INTO catalog_item_suppliers (parent, supplier, modified)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(parent, supplier) DO UPDATE SET modified = excluded.modified
            "#,
            params![
                link.parent.trim(),
                link.supplier.trim(),
                link.modified.trim()
            ],
        )?;
    }
    Ok(())
}

fn insert_item_customers(
    tx: &rusqlite::Transaction<'_>,
    links: &[CachedItemCustomer],
) -> rusqlite::Result<()> {
    for link in links {
        tx.execute(
            r#"
            INSERT INTO catalog_item_customers (parent, customer_name, modified)
            VALUES (?1, ?2, ?3)
            ON CONFLICT(parent, customer_name) DO UPDATE SET modified = excluded.modified
            "#,
            params![
                link.parent.trim(),
                link.customer_name.trim(),
                link.modified.trim()
            ],
        )?;
    }
    Ok(())
}

fn retain_single_key_table(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
    keys: &[String],
) -> rusqlite::Result<()> {
    if keys.is_empty() {
        tx.execute(&format!("DELETE FROM {table}"), [])?;
        return Ok(());
    }
    let placeholders = std::iter::repeat("?")
        .take(keys.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("DELETE FROM {table} WHERE {column} NOT IN ({placeholders})");
    tx.execute(&sql, params_from_iter(keys.iter().map(|key| key.trim())))?;
    Ok(())
}

fn retain_composite_key_table(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    left_column: &str,
    right_column: &str,
    temp_table: &str,
    keys: &[(String, String)],
) -> rusqlite::Result<()> {
    if keys.is_empty() {
        tx.execute(&format!("DELETE FROM {table}"), [])?;
        return Ok(());
    }

    tx.execute(
        &format!(
            "CREATE TEMP TABLE IF NOT EXISTS {temp_table} (left_key TEXT NOT NULL, right_key TEXT NOT NULL, PRIMARY KEY (left_key, right_key))"
        ),
        [],
    )?;
    tx.execute(&format!("DELETE FROM {temp_table}"), [])?;
    for (left, right) in keys {
        tx.execute(
            &format!("INSERT OR IGNORE INTO {temp_table} (left_key, right_key) VALUES (?1, ?2)"),
            params![left.trim(), right.trim()],
        )?;
    }
    tx.execute(
        &format!(
            "DELETE FROM {table}
             WHERE NOT EXISTS (
                 SELECT 1 FROM {temp_table} keys
                 WHERE keys.left_key = {table}.{left_column}
                   AND keys.right_key = {table}.{right_column}
             )"
        ),
        [],
    )?;
    tx.execute(&format!("DELETE FROM {temp_table}"), [])?;
    Ok(())
}

fn table_stats(conn: &Connection, table: &str) -> rusqlite::Result<CatalogTableStats> {
    conn.query_row(
        &format!("SELECT COUNT(*), COALESCE(MAX(modified), '') FROM {table}"),
        [],
        |row| {
            Ok(CatalogTableStats {
                count: row.get(0)?,
                max_modified: row.get(1)?,
            })
        },
    )
}

fn single_keys_missing<'a>(
    conn: &Connection,
    table: &str,
    column: &str,
    keys: impl Iterator<Item = &'a str>,
) -> rusqlite::Result<bool> {
    for key in keys {
        let exists = conn
            .query_row(
                &format!("SELECT 1 FROM {table} WHERE {column} = ?1 LIMIT 1"),
                params![key.trim()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Ok(true);
        }
    }
    Ok(false)
}

fn composite_keys_missing<'a>(
    conn: &Connection,
    table: &str,
    left_column: &str,
    right_column: &str,
    keys: impl Iterator<Item = (&'a str, &'a str)>,
) -> rusqlite::Result<bool> {
    for (left, right) in keys {
        let exists = conn
            .query_row(
                &format!(
                    "SELECT 1 FROM {table} WHERE {left_column} = ?1 AND {right_column} = ?2 LIMIT 1"
                ),
                params![left.trim(), right.trim()],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Ok(true);
        }
    }
    Ok(false)
}
