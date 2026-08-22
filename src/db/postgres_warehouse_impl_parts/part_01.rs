impl PostgresWarehouseStore {
    async fn warehouse(&self, warehouse: &str) -> Result<Option<AdminWarehouse>, WarehouseError> {
        let warehouse = warehouse.trim();
        if warehouse.is_empty() {
            return Ok(None);
        }
        let row = sqlx::query_as::<_, WarehouseRow>(
            "SELECT name, company, is_group, parent_warehouse
             FROM mini_warehouses
             WHERE lower(name) = lower($1)",
        )
        .bind(warehouse)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?;
        Ok(row.map(|row| AdminWarehouse {
            warehouse: row.name,
            company: row.company,
            is_group: row.is_group,
            parent_warehouse: row.parent_warehouse,
        }))
    }

    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, WarehouseError> {
        let query = query.trim().to_lowercase();
        let pattern = format!("%{query}%");
        let parent = parent.trim().to_lowercase();
        let rows = sqlx::query_as::<_, WarehouseRow>(
            "SELECT name, company, is_group, parent_warehouse
             FROM mini_warehouses
             WHERE ($1 = '' OR lower(name) LIKE $2)
               AND ($3 = '' OR lower(parent_warehouse) = $3)
             ORDER BY lower(name) ASC
             LIMIT $4",
        )
        .bind(query)
        .bind(pattern)
        .bind(parent)
        .bind(limit.max(1) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?;

        Ok(rows
            .into_iter()
            .map(|row| AdminWarehouse {
                warehouse: row.name,
                company: row.company,
                is_group: row.is_group,
                parent_warehouse: row.parent_warehouse,
            })
            .collect())
    }

    async fn put_warehouse(
        &self,
        warehouse: AdminWarehouse,
    ) -> Result<AdminWarehouse, WarehouseError> {
        let name = warehouse.warehouse.trim();
        if name.is_empty() {
            return Err(WarehouseError::MissingWarehouse);
        }
        sqlx::query_as::<_, WarehouseRow>(
            "INSERT INTO mini_warehouses (
                 id, name, company, is_group, parent_warehouse, payload_json
             )
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT ((lower(name))) DO UPDATE SET
               name = excluded.name,
               company = excluded.company,
               is_group = excluded.is_group,
               parent_warehouse = excluded.parent_warehouse,
               payload_json = excluded.payload_json,
               updated_at = now()
             RETURNING name, company, is_group, parent_warehouse",
        )
        .bind(warehouse_id(name))
        .bind(name)
        .bind(warehouse.company.trim())
        .bind(warehouse.is_group)
        .bind(warehouse.parent_warehouse.trim())
        .bind(serde_json::json!({
            "warehouse": name,
            "company": warehouse.company.trim(),
            "is_group": warehouse.is_group,
            "parent_warehouse": warehouse.parent_warehouse.trim(),
        }))
        .fetch_one(&self.pool)
        .await
        .map(|row| AdminWarehouse {
            warehouse: row.name,
            company: row.company,
            is_group: row.is_group,
            parent_warehouse: row.parent_warehouse,
        })
        .map_err(|_| WarehouseError::StoreFailed)
    }

    async fn warehouse_assignments(
        &self,
        warehouse: &str,
    ) -> Result<Vec<WarehouseAssignment>, WarehouseError> {
        let warehouse = warehouse.trim().to_lowercase();
        let rows = sqlx::query_as::<_, WarehouseAssignmentRow>(
            "SELECT assignment_kind, warehouse, warehouse_name, apparatus_id,
                    principal_role, principal_ref, display_name
             FROM mini_warehouse_assignments
             WHERE assignment_kind = 'warehouse'
               AND ($1 = '' OR lower(warehouse_name) = $1)
             ORDER BY lower(warehouse_name), lower(display_name), lower(principal_ref)",
        )
        .bind(warehouse)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?;

        rows.into_iter().map(row_to_assignment).collect()
    }

    async fn all_warehouse_assignments(&self) -> Result<Vec<WarehouseAssignment>, WarehouseError> {
        let rows = sqlx::query_as::<_, WarehouseAssignmentRow>(
            "SELECT assignment_kind, warehouse, warehouse_name, apparatus_id,
                    principal_role, principal_ref, display_name
             FROM mini_warehouse_assignments
             ORDER BY lower(assignment_kind),
                      lower(COALESCE(warehouse_name, apparatus_id)),
                      lower(display_name), lower(principal_ref)",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?;

        rows.into_iter().map(row_to_assignment).collect()
    }

    async fn warehouse_summaries(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WarehouseSummary>, WarehouseError> {
        let query = query.trim().to_lowercase();
        let pattern = format!("%{query}%");
        let rows = sqlx::query_as::<_, WarehouseSummaryRow>(
            r#"
            WITH raw_counts AS (
                SELECT stock.warehouse, count(*)::bigint AS raw_count
                FROM mini_raw_material_stock stock
                LEFT JOIN mini_inventory_placements placement
                  ON placement.asset_kind = 'raw_material'
                 AND lower(placement.asset_ref) = lower(stock.id)
                LEFT JOIN mini_inventory_locations physical_location
                  ON physical_location.id = placement.physical_location_id
                LEFT JOIN mini_warehouses physical_warehouse
                  ON physical_warehouse.id = physical_location.warehouse_id
                WHERE stock.status = 'available' AND stock.qty > 0
                  AND (
                        placement.asset_ref IS NULL
                        OR (
                            physical_location.kind = 'warehouse'
                            AND lower(physical_warehouse.name) = lower(stock.warehouse)
                        )
                  )
                GROUP BY stock.warehouse
            ),
            finished_counts AS (
                SELECT
                    stock.warehouse,
                    count(DISTINCT (lower(stock.item_code), lower(stock.uom)))::bigint AS finished_count
                FROM mini_finished_goods_stock stock
                LEFT JOIN mini_inventory_placements placement
                  ON placement.asset_kind = 'finished_goods'
                 AND lower(placement.asset_ref) = lower(stock.id)
                LEFT JOIN mini_inventory_locations physical_location
                  ON physical_location.id = placement.physical_location_id
                LEFT JOIN mini_warehouses physical_warehouse
                  ON physical_warehouse.id = physical_location.warehouse_id
                WHERE stock.status = 'available' AND stock.qty > 0
                  AND (
                        placement.asset_ref IS NULL
                        OR (
                            physical_location.kind = 'warehouse'
                            AND lower(physical_warehouse.name) = lower(stock.warehouse)
                        )
                  )
                GROUP BY stock.warehouse
            ),
            qolip_counts AS (
                SELECT stock.warehouse, COALESCE(sum(stock.quantity), 0)::bigint AS qolip_count
                FROM mini_qolip_locations stock
                LEFT JOIN mini_inventory_placements placement
                  ON placement.asset_kind = 'qolip'
                 AND lower(placement.asset_ref) = lower(stock.id)
                LEFT JOIN mini_inventory_locations physical_location
                  ON physical_location.id = placement.physical_location_id
                LEFT JOIN mini_warehouses physical_warehouse
                  ON physical_warehouse.id = physical_location.warehouse_id
                WHERE btrim(stock.warehouse) <> ''
                  AND btrim(COALESCE(stock.inventory_transfer_id, '')) = ''
                  AND (
                        placement.asset_ref IS NULL
                        OR (
                            physical_location.kind = 'warehouse'
                            AND lower(physical_warehouse.name) = lower(stock.warehouse)
                        )
                  )
                GROUP BY stock.warehouse
            ),
            qolip_checkout_counts AS (
                SELECT warehouse, COALESCE(sum(quantity), 0)::bigint AS checkout_count
                FROM mini_qolip_checkouts
                WHERE lower(status) = 'open' AND btrim(warehouse) <> ''
                GROUP BY warehouse
            ),
            reservation_counts AS (
                SELECT
                    stock.warehouse,
                    count(*)::bigint AS reserved_count
                FROM mini_raw_material_assignments assignments
                JOIN mini_raw_material_stock stock
                    ON lower(stock.barcode) = lower(assignments.barcode)
                WHERE btrim(stock.warehouse) <> ''
                GROUP BY stock.warehouse
            ),
            transfer_counts AS (
                SELECT warehouse, count(*)::bigint AS transfer_count
                FROM (
                    SELECT source_warehouse AS warehouse
                    FROM mini_inventory_transfers
                    WHERE status IN ('requested', 'approved', 'in_transit')
                    UNION ALL
                    SELECT destination_warehouse AS warehouse
                    FROM mini_inventory_transfers
                    WHERE status IN ('requested', 'approved', 'in_transit')
                ) active_transfers
                GROUP BY warehouse
            ),
            assignment_counts AS (
                SELECT
                    warehouse_name AS warehouse,
                    count(*)::bigint AS assignment_count,
                    string_agg(COALESCE(NULLIF(btrim(display_name), ''), principal_ref), E'\n'
                        ORDER BY lower(COALESCE(NULLIF(btrim(display_name), ''), principal_ref))) AS assigned_display_names
                FROM mini_warehouse_assignments
                WHERE assignment_kind = 'warehouse'
                GROUP BY warehouse_name
            ),
            warehouse_names AS (
                SELECT name AS warehouse
                FROM mini_warehouses
                WHERE btrim(parent_warehouse) = ''
                UNION
                SELECT warehouse FROM raw_counts
                UNION
                SELECT warehouse FROM finished_counts
                UNION
                SELECT warehouse FROM qolip_counts
                UNION
                SELECT warehouse FROM qolip_checkout_counts
                UNION
                SELECT warehouse FROM reservation_counts WHERE btrim(COALESCE(warehouse, '')) <> ''
                UNION
                SELECT warehouse FROM transfer_counts
                UNION
                SELECT warehouse FROM assignment_counts
            )
            SELECT
                warehouse_names.warehouse,
                (
                    COALESCE(raw_counts.raw_count, 0)
                    + COALESCE(finished_counts.finished_count, 0)
                    + COALESCE(qolip_counts.qolip_count, 0)
                )::bigint AS product_count,
                (
                    COALESCE(reservation_counts.reserved_count, 0)
                    + COALESCE(qolip_checkout_counts.checkout_count, 0)
                    + COALESCE(transfer_counts.transfer_count, 0)
                )::bigint AS reserved_count,
                COALESCE(assignment_counts.assignment_count, 0)::bigint AS assignment_count,
                COALESCE(assignment_counts.assigned_display_names, '') AS assigned_display_names
            FROM warehouse_names
            LEFT JOIN raw_counts ON lower(raw_counts.warehouse) = lower(warehouse_names.warehouse)
            LEFT JOIN finished_counts ON lower(finished_counts.warehouse) = lower(warehouse_names.warehouse)
            LEFT JOIN qolip_counts ON lower(qolip_counts.warehouse) = lower(warehouse_names.warehouse)
            LEFT JOIN qolip_checkout_counts ON lower(qolip_checkout_counts.warehouse) = lower(warehouse_names.warehouse)
            LEFT JOIN reservation_counts ON lower(reservation_counts.warehouse) = lower(warehouse_names.warehouse)
            LEFT JOIN transfer_counts ON lower(transfer_counts.warehouse) = lower(warehouse_names.warehouse)
            LEFT JOIN assignment_counts ON lower(assignment_counts.warehouse) = lower(warehouse_names.warehouse)
            WHERE ($1 = '' OR lower(warehouse_names.warehouse) LIKE $2)
            ORDER BY lower(warehouse_names.warehouse)
            LIMIT $3
            "#,
        )
        .bind(query)
        .bind(pattern)
        .bind(limit.clamp(1, 500) as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?;
        Ok(rows.into_iter().map(row_to_summary).collect())
    }

    async fn warehouse_stock_items(
        &self,
        warehouse: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<WarehouseStockItem>, WarehouseError> {
        let warehouse = warehouse.trim();
        if warehouse.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let needle = format!("%{}%", query.trim().to_lowercase());
        let rows = sqlx::query_as::<_, WarehouseStockItemRow>(
            r#"
            SELECT
                MAX(stock.item_code) AS code,
                COALESCE(
                    MAX(NULLIF(btrim(stock.item_name), '')),
                    MAX(NULLIF(btrim(items.name), '')),
                    MAX(stock.item_code)
                ) AS name,
                COALESCE(MAX(NULLIF(btrim(stock.uom), '')), MAX(NULLIF(btrim(items.uom), '')), '') AS uom,
                MAX(stock.warehouse) AS warehouse,
                COALESCE(MAX(NULLIF(btrim(items.item_group), '')), '') AS item_group,
                SUM(stock.qty)::float8 AS on_hand_qty,
                COUNT(*)::bigint AS package_count
            FROM mini_finished_goods_stock stock
            LEFT JOIN mini_items items ON lower(items.code) = lower(stock.item_code)
            LEFT JOIN mini_inventory_placements placement
              ON placement.asset_kind = 'finished_goods'
             AND lower(placement.asset_ref) = lower(stock.id)
            LEFT JOIN mini_inventory_locations physical_location
              ON physical_location.id = placement.physical_location_id
            LEFT JOIN mini_warehouses physical_warehouse
              ON physical_warehouse.id = physical_location.warehouse_id
            WHERE lower(stock.warehouse) = lower($1)
              AND stock.status = 'available'
              AND stock.qty > 0
              AND (
                    placement.asset_ref IS NULL
                    OR (
                        physical_location.kind = 'warehouse'
                        AND lower(physical_warehouse.name) = lower(stock.warehouse)
                    )
              )
              AND (
                    $2 = '%%'
                    OR lower(stock.item_code) LIKE $2
                    OR lower(stock.item_name) LIKE $2
                    OR lower(COALESCE(items.name, '')) LIKE $2
                    OR lower(COALESCE(items.item_group, '')) LIKE $2
              )
            GROUP BY lower(stock.item_code), lower(stock.uom), lower(stock.warehouse)
            ORDER BY lower(COALESCE(MAX(NULLIF(btrim(stock.item_name), '')), MAX(NULLIF(btrim(items.name), '')), MAX(stock.item_code)))
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(warehouse)
        .bind(needle)
        .bind(limit.min(500) as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?;

        Ok(rows.into_iter().map(row_to_stock_item).collect())
    }
}
