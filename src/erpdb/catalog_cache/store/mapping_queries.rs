use super::*;

impl CatalogCacheStore {
    pub fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 200, 500);
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT i.name, i.item_name, i.stock_uom, i.item_group
            FROM catalog_item_suppliers AS isup INDEXED BY idx_catalog_item_suppliers_supplier
            CROSS JOIN catalog_items i ON i.name = isup.parent
            WHERE isup.supplier = ?1
              AND i.disabled = 0
              AND i.is_stock_item = 1
            ORDER BY i.item_name COLLATE ERP_CATALOG ASC, i.name COLLATE ERP_CATALOG ASC
            LIMIT ?2
            "#,
        )?;
        let rows = stmt.query_map(params![supplier_ref.trim(), limit as i64], |row| {
            supplier_item_from_row(row, default_warehouse)
        })?;
        collect_rows(rows)
    }

    pub fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        if query.trim().is_empty() {
            return self.customer_items_page(customer_ref, limit, offset, default_warehouse);
        }
        let entries = self.customer_item_search_entries(customer_ref, default_warehouse)?;
        Ok(slice_page(
            &rank_customer_item_entries_by_query(entries, query),
            offset,
            limit,
        ))
    }

    pub fn werka_customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        let mut items =
            self.customer_items(customer_ref, query, limit, offset, default_warehouse)?;
        clear_item_groups(&mut items);
        Ok(items)
    }

    pub fn supplier_items(
        &self,
        supplier_ref: &str,
        query: &str,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let mut items = self.supplier_items_all(supplier_ref, default_warehouse)?;
        clear_item_groups(&mut items);
        if query.trim().is_empty() {
            return Ok(slice_page(&items, offset, limit));
        }
        Ok(slice_page(
            &rank_supplier_items_by_query(items, query),
            offset,
            limit,
        ))
    }

    pub fn customer_item_options(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<CustomerItemOption>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        if query.trim().is_empty() {
            return self.customer_item_options_page(limit, offset, default_warehouse);
        }
        let items = self.customer_item_options_all(default_warehouse)?;
        Ok(slice_page(
            &rank_customer_item_options_by_query(items, query),
            offset,
            limit,
        ))
    }

    pub fn werka_suppliers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierDirectoryEntry>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let like = sqlite_like_pattern(query);
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT s.name, s.supplier_name, s.mobile_no
            FROM catalog_suppliers s
            WHERE s.disabled = 0
              AND (?1 = '' OR s.name LIKE ?2 ESCAPE '\' OR s.supplier_name LIKE ?2 ESCAPE '\' OR s.mobile_no LIKE ?2 ESCAPE '\')
              AND EXISTS (
                  SELECT 1
                  FROM catalog_item_suppliers AS isup INDEXED BY idx_catalog_item_suppliers_supplier
                  CROSS JOIN catalog_items i ON i.name = isup.parent
                  WHERE isup.supplier = s.name
                    AND i.disabled = 0
                    AND i.is_stock_item = 1
                  LIMIT 1
              )
            ORDER BY s.modified DESC, s.supplier_name COLLATE ERP_CATALOG ASC, s.name COLLATE ERP_CATALOG ASC
            LIMIT ?3 OFFSET ?4
            "#,
        )?;
        let rows = stmt.query_map(
            params![query.trim(), like, limit as i64, offset as i64],
            |row| {
                Ok(SupplierDirectoryEntry {
                    ref_: row.get::<_, String>(0)?.trim().to_string(),
                    name: blank_default(&row.get::<_, String>(1)?, &row.get::<_, String>(0)?),
                    phone: row.get::<_, String>(2)?.trim().to_string(),
                })
            },
        )?;
        collect_rows(rows)
    }

    pub fn werka_customers(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let like = sqlite_like_pattern(query);
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT c.name, c.customer_name, c.mobile_no
            FROM catalog_customers c
            WHERE c.disabled = 0
              AND (?1 = '' OR c.name LIKE ?2 ESCAPE '\' OR c.customer_name LIKE ?2 ESCAPE '\' OR c.mobile_no LIKE ?2 ESCAPE '\')
              AND EXISTS (
                  SELECT 1
                  FROM catalog_item_customers AS icd INDEXED BY idx_catalog_item_customers_customer
                  CROSS JOIN catalog_items i ON i.name = icd.parent
                  WHERE icd.customer_name = c.name
                    AND i.disabled = 0
                    AND i.is_stock_item = 1
                  LIMIT 1
              )
            ORDER BY c.modified DESC, c.customer_name COLLATE ERP_CATALOG ASC, c.name COLLATE ERP_CATALOG ASC
            LIMIT ?3 OFFSET ?4
            "#,
        )?;
        let rows = stmt.query_map(
            params![query.trim(), like, limit as i64, offset as i64],
            |row| {
                Ok(CustomerDirectoryEntry {
                    ref_: row.get::<_, String>(0)?.trim().to_string(),
                    name: blank_default(&row.get::<_, String>(1)?, &row.get::<_, String>(0)?),
                    phone: row.get::<_, String>(2)?.trim().to_string(),
                })
            },
        )?;
        collect_rows(rows)
    }

    fn customer_items_page(
        &self,
        customer_ref: &str,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT i.name, i.item_name, i.stock_uom, i.item_group
            FROM catalog_item_customers icd
            INNER JOIN catalog_items i ON i.name = icd.parent
            WHERE icd.customer_name = ?1
              AND i.disabled = 0
              AND i.is_stock_item = 1
            ORDER BY i.item_name COLLATE ERP_CATALOG ASC, i.name COLLATE ERP_CATALOG ASC
            LIMIT ?2 OFFSET ?3
            "#,
        )?;
        let rows = stmt.query_map(
            params![customer_ref.trim(), limit as i64, offset as i64],
            |row| supplier_item_from_row(row, default_warehouse),
        )?;
        collect_rows(rows)
    }

    fn supplier_items_all(
        &self,
        supplier_ref: &str,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT i.name, i.item_name, i.stock_uom, i.item_group
            FROM catalog_item_suppliers AS isup INDEXED BY idx_catalog_item_suppliers_supplier
            CROSS JOIN catalog_items i ON i.name = isup.parent
            WHERE isup.supplier = ?1
              AND i.disabled = 0
              AND i.is_stock_item = 1
            ORDER BY i.item_name COLLATE ERP_CATALOG ASC, i.name COLLATE ERP_CATALOG ASC
            "#,
        )?;
        let rows = stmt.query_map(params![supplier_ref.trim()], |row| {
            supplier_item_from_row(row, default_warehouse)
        })?;
        collect_rows(rows)
    }

    fn customer_item_search_entries(
        &self,
        customer_ref: &str,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItemSearchEntry>, CatalogCacheError> {
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT i.name, i.item_name, i.stock_uom, i.item_group
            FROM catalog_item_customers icd
            INNER JOIN catalog_items i ON i.name = icd.parent
            WHERE icd.customer_name = ?1
              AND i.disabled = 0
              AND i.is_stock_item = 1
            ORDER BY i.item_name COLLATE ERP_CATALOG ASC, i.name COLLATE ERP_CATALOG ASC
            "#,
        )?;
        let rows = stmt.query_map(params![customer_ref.trim()], |row| {
            let item = supplier_item_from_row(row, default_warehouse)?;
            Ok(SupplierItemSearchEntry {
                search_terms: vec![item.code.clone(), item.name.clone()],
                item,
            })
        })?;
        collect_rows(rows)
    }

    fn customer_item_options_all(
        &self,
        default_warehouse: &str,
    ) -> Result<Vec<CustomerItemOption>, CatalogCacheError> {
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT
                c.name,
                c.customer_name,
                c.mobile_no,
                i.name,
                i.item_name,
                i.stock_uom
            FROM catalog_item_customers icd
            INNER JOIN catalog_customers c ON c.name = icd.customer_name
            INNER JOIN catalog_items i ON i.name = icd.parent
            WHERE c.disabled = 0
              AND i.disabled = 0
              AND i.is_stock_item = 1
            ORDER BY i.item_name COLLATE ERP_CATALOG ASC, c.customer_name COLLATE ERP_CATALOG ASC, i.name COLLATE ERP_CATALOG ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            customer_item_option_from_row(row, default_warehouse)
        })?;
        collect_rows(rows)
    }

    fn customer_item_options_page(
        &self,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<CustomerItemOption>, CatalogCacheError> {
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT DISTINCT
                c.name,
                c.customer_name,
                c.mobile_no,
                i.name,
                i.item_name,
                i.stock_uom
            FROM catalog_item_customers icd
            INNER JOIN catalog_customers c ON c.name = icd.customer_name
            INNER JOIN catalog_items i ON i.name = icd.parent
            WHERE c.disabled = 0
              AND i.disabled = 0
              AND i.is_stock_item = 1
            ORDER BY i.item_name COLLATE ERP_CATALOG ASC, c.customer_name COLLATE ERP_CATALOG ASC, i.name COLLATE ERP_CATALOG ASC
            LIMIT ?1 OFFSET ?2
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], |row| {
            customer_item_option_from_row(row, default_warehouse)
        })?;
        collect_rows(rows)
    }
}

fn clear_item_groups(items: &mut [SupplierItem]) {
    for item in items {
        item.item_group.clear();
    }
}

fn customer_item_option_from_row(
    row: &rusqlite::Row<'_>,
    default_warehouse: &str,
) -> rusqlite::Result<CustomerItemOption> {
    let customer_ref: String = row.get(0)?;
    let customer_name: String = row.get(1)?;
    let item_code: String = row.get(3)?;
    let item_name: String = row.get(4)?;
    Ok(CustomerItemOption {
        customer_ref: customer_ref.trim().to_string(),
        customer_name: blank_default(&customer_name, &customer_ref),
        customer_phone: row.get::<_, String>(2)?.trim().to_string(),
        item_code: item_code.trim().to_string(),
        item_name: blank_default(&item_name, &item_code),
        uom: row.get::<_, String>(5)?.trim().to_string(),
        warehouse: default_warehouse.trim().to_string(),
    })
}
