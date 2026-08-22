
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
            "SELECT items.code, items.name, items.uom, items.item_group,
                    COALESCE((
                        SELECT array_agg(
                            CASE WHEN btrim(customers.name) <> ''
                                 THEN customers.name ELSE customers.ref END
                            ORDER BY lower(customers.name), customers.ref
                        )
                        FROM mini_customer_items assignments
                        JOIN mini_customers customers
                          ON customers.ref = assignments.customer_ref
                        WHERE assignments.item_code = items.code
                    ), ARRAY[]::text[]) AS customer_names
             FROM mini_items items
             WHERE $1 = '%%'
                OR lower(items.code) LIKE $1
                OR lower(items.name) LIKE $1
                OR lower(items.item_group) LIKE $1
                OR EXISTS (
                    SELECT 1
                    FROM mini_customer_items assignments
                    JOIN mini_customers customers
                      ON customers.ref = assignments.customer_ref
                    WHERE assignments.item_code = items.code
                      AND lower(customers.name) LIKE $1
                )
             ORDER BY lower(items.code)
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

    async fn item_uoms(&self) -> Result<Vec<String>, AdminPortError> {
        sqlx::query_scalar::<_, String>(
            "SELECT min(btrim(uom)) AS uom
             FROM mini_items
             WHERE btrim(uom) <> ''
             GROUP BY lower(btrim(uom))
             ORDER BY lower(min(btrim(uom)))",
        )
        .fetch_all(&self.pool)
        .await
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
            "SELECT items.code, items.name, items.uom, items.item_group,
                    COALESCE((
                        SELECT array_agg(
                            CASE WHEN btrim(customers.name) <> ''
                                 THEN customers.name ELSE customers.ref END
                            ORDER BY lower(customers.name), customers.ref
                        )
                        FROM mini_customer_items assignments
                        JOIN mini_customers customers
                          ON customers.ref = assignments.customer_ref
                        WHERE assignments.item_code = items.code
                    ), ARRAY[]::text[]) AS customer_names
             FROM mini_items items
             WHERE lower(items.item_group) = lower($1)
               AND (
                    $2 = '%%'
                    OR lower(items.code) LIKE $2
                    OR lower(items.name) LIKE $2
                    OR lower(items.item_group) LIKE $2
                    OR EXISTS (
                        SELECT 1
                        FROM mini_customer_items assignments
                        JOIN mini_customers customers
                          ON customers.ref = assignments.customer_ref
                        WHERE assignments.item_code = items.code
                          AND lower(customers.name) LIKE $2
                    )
               )
             ORDER BY lower(items.code)
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

    async fn items_page_in_groups(
        &self,
        groups: &[String],
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let mut groups = groups
            .iter()
            .map(|group| group.trim().to_lowercase())
            .filter(|group| !group.is_empty())
            .collect::<Vec<_>>();
        groups.sort_unstable();
        groups.dedup();
        if groups.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let needle = format!("%{}%", query.trim().to_lowercase());
        sqlx::query_as::<_, ItemRow>(
            "SELECT items.code, items.name, items.uom, items.item_group,
                    COALESCE((
                        SELECT array_agg(
                            CASE WHEN btrim(customers.name) <> ''
                                 THEN customers.name ELSE customers.ref END
                            ORDER BY lower(customers.name), customers.ref
                        )
                        FROM mini_customer_items assignments
                        JOIN mini_customers customers
                          ON customers.ref = assignments.customer_ref
                        WHERE assignments.item_code = items.code
                    ), ARRAY[]::text[]) AS customer_names
             FROM mini_items items
             WHERE lower(items.item_group) = ANY($1::text[])
               AND (
                    $2 = '%%'
                    OR lower(items.code) LIKE $2
                    OR lower(items.name) LIKE $2
                    OR lower(items.item_group) LIKE $2
                    OR EXISTS (
                        SELECT 1
                        FROM mini_customer_items assignments
                        JOIN mini_customers customers
                          ON customers.ref = assignments.customer_ref
                        WHERE assignments.item_code = items.code
                          AND lower(customers.name) LIKE $2
                    )
               )
             ORDER BY lower(items.code), items.code
             LIMIT $3 OFFSET $4",
        )
        .bind(groups)
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
            "SELECT items.code, items.name, items.uom, items.item_group,
                    COALESCE((
                        SELECT array_agg(
                            CASE WHEN btrim(customers.name) <> ''
                                 THEN customers.name ELSE customers.ref END
                            ORDER BY lower(customers.name), customers.ref
                        )
                        FROM mini_customer_items assignments
                        JOIN mini_customers customers
                          ON customers.ref = assignments.customer_ref
                        WHERE assignments.item_code = items.code
                    ), ARRAY[]::text[]) AS customer_names
             FROM mini_items items
             WHERE lower(items.code) = ANY($1)
             ORDER BY lower(items.code)",
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
