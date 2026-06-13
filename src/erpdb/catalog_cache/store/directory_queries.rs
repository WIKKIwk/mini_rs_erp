use super::*;

impl CatalogCacheStore {
    pub fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let like = sqlite_like_pattern(query);
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT name, supplier_name, mobile_no
            FROM catalog_suppliers
            WHERE disabled = 0
              AND (?1 = '' OR name LIKE ?2 ESCAPE '\' OR supplier_name LIKE ?2 ESCAPE '\' OR mobile_no LIKE ?2 ESCAPE '\')
            ORDER BY modified DESC, supplier_name COLLATE ERP_CATALOG ASC, name COLLATE ERP_CATALOG ASC
            LIMIT ?3 OFFSET ?4
            "#,
        )?;
        let rows = stmt.query_map(
            params![query.trim(), like, limit as i64, offset as i64],
            |row| admin_supplier_from_row(row),
        )?;
        collect_rows(rows)
    }

    pub fn supplier_by_ref(
        &self,
        ref_: &str,
    ) -> Result<Option<AdminDirectoryEntry>, CatalogCacheError> {
        self.ensure_ready()?;
        let conn = self.lock_read()?;
        conn.query_row(
            r#"
            SELECT name, supplier_name, mobile_no
            FROM catalog_suppliers
            WHERE disabled = 0 AND name = ?1
            LIMIT 1
            "#,
            params![ref_.trim()],
            admin_supplier_from_row,
        )
        .optional()
        .map_err(CatalogCacheError::from)
    }

    pub fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let like = sqlite_like_pattern(query);
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT name, customer_name, mobile_no
            FROM catalog_customers
            WHERE disabled = 0
              AND (?1 = '' OR name LIKE ?2 ESCAPE '\' OR customer_name LIKE ?2 ESCAPE '\' OR mobile_no LIKE ?2 ESCAPE '\')
            ORDER BY modified DESC, customer_name COLLATE ERP_CATALOG ASC, name COLLATE ERP_CATALOG ASC
            LIMIT ?3 OFFSET ?4
            "#,
        )?;
        let rows = stmt.query_map(
            params![query.trim(), like, limit as i64, offset as i64],
            |row| admin_customer_from_row(row),
        )?;
        collect_rows(rows)
    }

    pub fn customer_by_ref(
        &self,
        ref_: &str,
    ) -> Result<Option<AdminDirectoryEntry>, CatalogCacheError> {
        self.ensure_ready()?;
        let conn = self.lock_read()?;
        conn.query_row(
            r#"
            SELECT name, customer_name, mobile_no
            FROM catalog_customers
            WHERE disabled = 0 AND name = ?1
            LIMIT 1
            "#,
            params![ref_.trim()],
            admin_customer_from_row,
        )
        .optional()
        .map_err(CatalogCacheError::from)
    }

    pub fn supplier_profile(
        &self,
        id: &str,
    ) -> Result<Option<SupplierProfileRecord>, CatalogCacheError> {
        self.ensure_ready()?;
        let conn = self.lock_read()?;
        conn.query_row(
            r#"
            SELECT mobile_no, supplier_details, image
            FROM catalog_suppliers
            WHERE name = ?1
            LIMIT 1
            "#,
            params![id.trim()],
            |row| {
                Ok(SupplierProfileRecord {
                    phone: profile_phone(&row.get::<_, String>(0)?, &row.get::<_, String>(1)?),
                    image: row.get::<_, String>(2)?.trim().to_string(),
                })
            },
        )
        .optional()
        .map_err(CatalogCacheError::from)
    }

    pub fn customer_profile(
        &self,
        id: &str,
    ) -> Result<Option<CustomerProfileRecord>, CatalogCacheError> {
        self.ensure_ready()?;
        let conn = self.lock_read()?;
        conn.query_row(
            r#"
            SELECT mobile_no, customer_details
            FROM catalog_customers
            WHERE name = ?1
            LIMIT 1
            "#,
            params![id.trim()],
            |row| {
                Ok(CustomerProfileRecord {
                    phone: profile_phone(&row.get::<_, String>(0)?, &row.get::<_, String>(1)?),
                })
            },
        )
        .optional()
        .map_err(CatalogCacheError::from)
    }
}
fn admin_supplier_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AdminDirectoryEntry> {
    let ref_: String = row.get(0)?;
    let name: String = row.get(1)?;
    Ok(AdminDirectoryEntry {
        ref_: ref_.trim().to_string(),
        name: blank_default(&name, &ref_),
        phone: row.get::<_, String>(2)?.trim().to_string(),
    })
}

fn admin_customer_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AdminDirectoryEntry> {
    let ref_: String = row.get(0)?;
    let name: String = row.get(1)?;
    Ok(AdminDirectoryEntry {
        ref_: ref_.trim().to_string(),
        name: blank_default(&name, &ref_),
        phone: row.get::<_, String>(2)?.trim().to_string(),
    })
}
