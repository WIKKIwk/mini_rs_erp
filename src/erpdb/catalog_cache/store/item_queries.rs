use super::*;

impl CatalogCacheStore {
    pub fn items_page(
        &self,
        query: &str,
        group: Option<&str>,
        limit: usize,
        offset: usize,
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let group = group.map(str::trim).filter(|value| !value.is_empty());
        let conn = self.lock_read()?;
        let like = sqlite_like_pattern(query);
        let mut stmt = conn.prepare(match group {
            Some(_) => {
                r#"
                SELECT name, item_name, stock_uom, item_group
                FROM catalog_items
                WHERE disabled = 0
                  AND is_stock_item = 1
                  AND item_group = ?1
                  AND (?2 = '' OR name LIKE ?3 ESCAPE '\' OR item_name LIKE ?3 ESCAPE '\')
                ORDER BY item_name COLLATE ERP_CATALOG ASC, name COLLATE ERP_CATALOG ASC
                LIMIT ?4 OFFSET ?5
                "#
            }
            None => {
                r#"
                SELECT name, item_name, stock_uom, item_group
                FROM catalog_items
                WHERE disabled = 0
                  AND is_stock_item = 1
                  AND (?1 = '' OR name LIKE ?2 ESCAPE '\' OR item_name LIKE ?2 ESCAPE '\')
                ORDER BY item_name COLLATE ERP_CATALOG ASC, name COLLATE ERP_CATALOG ASC
                LIMIT ?3 OFFSET ?4
                "#
            }
        })?;
        match group {
            Some(group) => {
                let rows = stmt.query_map(
                    params![group, query.trim(), like, limit as i64, offset as i64],
                    |row| supplier_item_from_row(row, default_warehouse),
                )?;
                collect_rows(rows)
            }
            None => {
                let rows = stmt.query_map(
                    params![query.trim(), like, limit as i64, offset as i64],
                    |row| supplier_item_from_row(row, default_warehouse),
                )?;
                collect_rows(rows)
            }
        }
    }

    pub fn items_by_codes(
        &self,
        item_codes: &[String],
        default_warehouse: &str,
    ) -> Result<Vec<SupplierItem>, CatalogCacheError> {
        self.ensure_ready()?;
        let codes = item_codes
            .iter()
            .map(|code| code.trim().to_string())
            .filter(|code| !code.is_empty())
            .take(500)
            .collect::<Vec<_>>();
        if codes.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = std::iter::repeat_n("?", codes.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            SELECT name, item_name, stock_uom, item_group
            FROM catalog_items
            WHERE disabled = 0
              AND is_stock_item = 1
              AND name IN ({placeholders})
            ORDER BY item_name COLLATE ERP_CATALOG ASC, name COLLATE ERP_CATALOG ASC
            "#
        );
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(codes.iter()), |row| {
            supplier_item_from_row(row, default_warehouse)
        })?;
        collect_rows(rows)
    }

    pub fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, CatalogCacheError> {
        self.ensure_ready()?;
        let limit = clamp_limit(limit, 50, 500);
        let like = sqlite_like_pattern(query);
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT name
            FROM catalog_item_groups
            WHERE ?1 = '' OR name LIKE ?2 ESCAPE '\' OR item_group_name LIKE ?2 ESCAPE '\'
            ORDER BY name ASC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt.query_map(params![query.trim(), like, limit as i64], |row| {
            row.get::<_, String>(0)
        })?;
        collect_rows(rows).map(|groups| {
            groups
                .into_iter()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect()
        })
    }

    pub fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, CatalogCacheError> {
        self.ensure_ready()?;
        let conn = self.lock_read()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT name, item_group_name, parent_item_group, is_group
            FROM catalog_item_groups
            ORDER BY lft ASC, name ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            let name: String = row.get(0)?;
            let item_group_name: String = row.get(1)?;
            Ok(AdminItemGroup {
                name: name.trim().to_string(),
                item_group_name: blank_default(&item_group_name, &name),
                parent_item_group: row.get::<_, String>(2)?.trim().to_string(),
                is_group: row.get::<_, i64>(3)? != 0,
            })
        })?;
        collect_rows(rows)
    }
}
