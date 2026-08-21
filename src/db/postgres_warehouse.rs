use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::admin::models::AdminWarehouse;
use crate::core::auth::models::PrincipalRole;
use crate::core::warehouses::{
    WarehouseAssignment, WarehouseAssignmentIdentity, WarehouseDeleteResult, WarehouseError,
    WarehouseStockItem, WarehouseStorePort, WarehouseSummary,
};

#[derive(Clone)]
pub struct PostgresWarehouseStore {
    pool: PgPool,
}

impl PostgresWarehouseStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WarehouseStorePort for PostgresWarehouseStore {
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

    async fn put_warehouse_assignment(
        &self,
        assignment: WarehouseAssignment,
    ) -> Result<WarehouseAssignment, WarehouseError> {
        let warehouse = assignment.warehouse.trim();
        let principal_ref = assignment.principal_ref.trim();
        if warehouse.is_empty() {
            return Err(WarehouseError::MissingWarehouse);
        }
        if principal_ref.is_empty() {
            return Err(WarehouseError::MissingPrincipalRef);
        }
        let assignment_kind = assignment.assignment_kind.trim();
        let conflict_target = match assignment_kind {
            "warehouse" => {
                "(warehouse_name, principal_role, principal_ref) WHERE assignment_kind = 'warehouse'"
            }
            "apparatus" => {
                "(apparatus_id, principal_role, principal_ref) WHERE assignment_kind = 'apparatus'"
            }
            _ => return Err(WarehouseError::StoreFailed),
        };
        let query = format!(
            "INSERT INTO mini_warehouse_assignments (
                 assignment_kind, warehouse, warehouse_name, apparatus_id,
                 principal_role, principal_ref, display_name, payload_json
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT {conflict_target} DO UPDATE SET
               warehouse = excluded.warehouse,
               warehouse_name = excluded.warehouse_name,
               apparatus_id = excluded.apparatus_id,
               display_name = excluded.display_name,
               payload_json = excluded.payload_json,
               updated_at = now()
             WHERE mini_warehouse_assignments.assignment_kind = excluded.assignment_kind
               AND (
                    (excluded.assignment_kind = 'warehouse'
                     AND mini_warehouse_assignments.warehouse_name = excluded.warehouse_name)
                    OR
                    (excluded.assignment_kind = 'apparatus'
                     AND mini_warehouse_assignments.apparatus_id = excluded.apparatus_id)
               )
             RETURNING assignment_kind, warehouse, warehouse_name, apparatus_id,
                       principal_role, principal_ref, display_name"
        );
        sqlx::query_as::<_, WarehouseAssignmentRow>(&query)
            .bind(assignment_kind)
            .bind(warehouse)
            .bind(assignment.warehouse_name.as_deref().map(str::trim))
            .bind(assignment.apparatus_id.as_deref().map(str::trim))
            .bind(role_as_str(&assignment.principal_role))
            .bind(principal_ref)
            .bind(assignment.display_name.trim())
            .bind(serde_json::json!({
                "assignment_kind": assignment_kind,
                "warehouse": warehouse,
                "warehouse_name": assignment.warehouse_name.as_deref().map(str::trim),
                "apparatus_id": assignment.apparatus_id.as_deref().map(str::trim),
                "principal_role": role_as_str(&assignment.principal_role),
                "principal_ref": principal_ref,
                "display_name": assignment.display_name.trim(),
            }))
            .fetch_one(&self.pool)
            .await
            .map_err(|_| WarehouseError::StoreFailed)
            .and_then(row_to_assignment)
    }

    async fn delete_warehouse_assignment(
        &self,
        identity: &WarehouseAssignmentIdentity,
        principal_role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<Option<WarehouseAssignment>, WarehouseError> {
        let (query, identity_value) = match identity {
            WarehouseAssignmentIdentity::WarehouseName(warehouse) => (
                "DELETE FROM mini_warehouse_assignments
                 WHERE assignment_kind = 'warehouse'
                   AND lower(warehouse_name) = lower($1)
                   AND principal_role = $2
                   AND principal_ref = $3
                 RETURNING assignment_kind, warehouse, warehouse_name, apparatus_id,
                           principal_role, principal_ref, display_name",
                warehouse.as_str(),
            ),
            WarehouseAssignmentIdentity::ApparatusId(apparatus_id) => (
                "DELETE FROM mini_warehouse_assignments
                 WHERE assignment_kind = 'apparatus'
                   AND apparatus_id = $1
                   AND principal_role = $2
                   AND principal_ref = $3
                 RETURNING assignment_kind, warehouse, warehouse_name, apparatus_id,
                           principal_role, principal_ref, display_name",
                apparatus_id.as_str(),
            ),
        };
        let row = sqlx::query_as::<_, WarehouseAssignmentRow>(query)
            .bind(identity_value)
            .bind(role_as_str(principal_role))
            .bind(principal_ref.trim())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;

        row.map(row_to_assignment).transpose()
    }

    async fn delete_warehouse(
        &self,
        warehouse: &str,
        delete_products: bool,
    ) -> Result<WarehouseDeleteResult, WarehouseError> {
        let warehouse = warehouse.trim();
        if warehouse.is_empty() {
            return Err(WarehouseError::MissingWarehouse);
        }
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        // Transfer creation holds FOR KEY SHARE on these same rows until its
        // transaction commits, so the delete recheck cannot miss a new transfer.
        let (warehouse_id, warehouse_name) = sqlx::query_as::<_, (String, String)>(
            "SELECT id, name
             FROM mini_warehouses
             WHERE lower(name) = lower($1)
             FOR UPDATE",
        )
        .bind(warehouse)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?
        .ok_or(WarehouseError::NotFound)?;
        let has_children = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                 SELECT 1 FROM mini_warehouses
                 WHERE lower(parent_warehouse) = lower($1)
             )",
        )
        .bind(&warehouse_name)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?;
        if has_children {
            return Err(WarehouseError::HasChildren);
        }
        let snapshot =
            warehouse_delete_snapshot_tx(&mut transaction, &warehouse_id, &warehouse_name).await?;
        let product_count = count_as_usize(snapshot.product_count);
        let reserved_count = count_as_usize(snapshot.reserved_count);
        let assignment_count = count_as_usize(snapshot.assignment_count);
        if reserved_count > 0 {
            return Err(WarehouseError::HasActiveReservations(reserved_count));
        }
        if product_count > 0 && !delete_products {
            return Err(WarehouseError::NotEmpty(product_count));
        }
        for table in ["mini_qolip_cell_qrs", "mini_qolip_locations"] {
            sqlx::query(&format!(
                "DELETE FROM {table}
                 WHERE lower(warehouse) = lower($1) OR lower(block) = lower($1)"
            ))
            .bind(&warehouse_name)
            .execute(&mut *transaction)
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        }
        for table in ["mini_raw_material_stock", "mini_finished_goods_stock"] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE lower(warehouse) = lower($1)"
            ))
            .bind(&warehouse_name)
            .execute(&mut *transaction)
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        }
        sqlx::query(
            "DELETE FROM mini_warehouse_assignments
                     WHERE assignment_kind = 'warehouse'
                       AND lower(warehouse_name) = lower($1)",
        )
        .bind(&warehouse_name)
        .execute(&mut *transaction)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?;
        let deleted = sqlx::query("DELETE FROM mini_warehouses WHERE id = $1")
            .bind(&warehouse_id)
            .execute(&mut *transaction)
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        if deleted.rows_affected() != 1 {
            return Err(WarehouseError::NotFound);
        }
        transaction
            .commit()
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        Ok(WarehouseDeleteResult {
            warehouse: warehouse_name,
            deleted_product_count: product_count,
            deleted_assignment_count: assignment_count,
        })
    }
}

async fn warehouse_delete_snapshot_tx(
    tx: &mut Transaction<'_, Postgres>,
    warehouse_id: &str,
    warehouse: &str,
) -> Result<WarehouseDeleteSnapshotRow, WarehouseError> {
    sqlx::query_as::<_, WarehouseDeleteSnapshotRow>(
        r#"
        SELECT
            (
                (SELECT count(*)
                 FROM mini_raw_material_stock stock
                 LEFT JOIN mini_inventory_placements placement
                   ON placement.asset_kind = 'raw_material'
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
                   ))
                +
                (SELECT count(DISTINCT (lower(item_code), lower(uom)))
                 FROM mini_finished_goods_stock stock
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
                   ))
                +
                (SELECT COALESCE(sum(stock.quantity), 0)
                 FROM mini_qolip_locations stock
                 LEFT JOIN mini_inventory_placements placement
                   ON placement.asset_kind = 'qolip'
                  AND lower(placement.asset_ref) = lower(stock.id)
                 LEFT JOIN mini_inventory_locations physical_location
                   ON physical_location.id = placement.physical_location_id
                 LEFT JOIN mini_warehouses physical_warehouse
                   ON physical_warehouse.id = physical_location.warehouse_id
                 WHERE lower(stock.warehouse) = lower($1)
                   AND (
                       placement.asset_ref IS NULL
                       OR (
                           physical_location.kind = 'warehouse'
                           AND lower(physical_warehouse.name) = lower(stock.warehouse)
                       )
                   ))
            )::bigint AS product_count,
            (
                (SELECT count(*)
                 FROM mini_raw_material_assignments assignment
                 JOIN mini_raw_material_stock stock
                   ON lower(stock.barcode) = lower(assignment.barcode)
                 WHERE lower(stock.warehouse) = lower($1))
                +
                (SELECT COALESCE(sum(GREATEST(quantity, 0)), 0)
                 FROM mini_qolip_checkouts
                 WHERE lower(status) = 'open' AND lower(warehouse) = lower($1))
                +
                (SELECT count(*)
                 FROM mini_inventory_transfers
                 WHERE status IN ('requested', 'approved', 'in_transit')
                   AND (
                       source_warehouse_id = $2
                       OR destination_warehouse_id = $2
                       OR lower(source_warehouse) = lower($1)
                       OR lower(destination_warehouse) = lower($1)
                   ))
            )::bigint AS reserved_count,
            (SELECT count(*)
             FROM mini_warehouse_assignments
             WHERE assignment_kind = 'warehouse'
               AND lower(warehouse_name) = lower($1))::bigint AS assignment_count
        "#,
    )
    .bind(warehouse)
    .bind(warehouse_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| WarehouseError::StoreFailed)
}

fn count_as_usize(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

#[derive(sqlx::FromRow)]
struct WarehouseRow {
    name: String,
    company: String,
    is_group: bool,
    parent_warehouse: String,
}

#[derive(sqlx::FromRow)]
struct WarehouseAssignmentRow {
    assignment_kind: String,
    warehouse: String,
    warehouse_name: Option<String>,
    apparatus_id: Option<String>,
    principal_role: String,
    principal_ref: String,
    display_name: String,
}

#[derive(sqlx::FromRow)]
struct WarehouseSummaryRow {
    warehouse: String,
    product_count: i64,
    reserved_count: i64,
    assignment_count: i64,
    assigned_display_names: String,
}

#[derive(sqlx::FromRow)]
struct WarehouseDeleteSnapshotRow {
    product_count: i64,
    reserved_count: i64,
    assignment_count: i64,
}

#[derive(sqlx::FromRow)]
struct WarehouseStockItemRow {
    code: String,
    name: String,
    uom: String,
    warehouse: String,
    item_group: String,
    on_hand_qty: f64,
    package_count: i64,
}

fn warehouse_id(name: &str) -> String {
    format!("warehouse:{}", name.trim().to_lowercase())
}

fn role_as_str(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Boyoqchi => "boyoqchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}

fn role_from_str(raw: &str) -> Result<PrincipalRole, WarehouseError> {
    match raw.trim().to_lowercase().as_str() {
        "supplier" => Ok(PrincipalRole::Supplier),
        "werka" => Ok(PrincipalRole::Werka),
        "customer" => Ok(PrincipalRole::Customer),
        "aparatchi" => Ok(PrincipalRole::Aparatchi),
        "qolipchi" => Ok(PrincipalRole::Qolipchi),
        "boyoqchi" => Ok(PrincipalRole::Boyoqchi),
        "material_taminotchi" => Ok(PrincipalRole::MaterialTaminotchi),
        "admin" => Ok(PrincipalRole::Admin),
        _ => Err(WarehouseError::StoreFailed),
    }
}

fn row_to_assignment(row: WarehouseAssignmentRow) -> Result<WarehouseAssignment, WarehouseError> {
    Ok(WarehouseAssignment {
        assignment_kind: row.assignment_kind,
        warehouse: row.warehouse,
        warehouse_name: row.warehouse_name,
        apparatus_id: row.apparatus_id,
        principal_role: role_from_str(&row.principal_role)?,
        principal_ref: row.principal_ref,
        display_name: row.display_name,
    })
}

fn row_to_summary(row: WarehouseSummaryRow) -> WarehouseSummary {
    WarehouseSummary {
        warehouse: row.warehouse,
        product_count: row.product_count.max(0) as usize,
        reserved_count: row.reserved_count.max(0) as usize,
        assignment_count: row.assignment_count.max(0) as usize,
        assigned_display_names: row
            .assigned_display_names
            .lines()
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
    }
}

fn row_to_stock_item(row: WarehouseStockItemRow) -> WarehouseStockItem {
    WarehouseStockItem {
        code: row.code,
        name: row.name,
        uom: row.uom,
        warehouse: row.warehouse,
        item_group: row.item_group,
        on_hand_qty: row.on_hand_qty.max(0.0),
        package_count: row.package_count.max(0) as usize,
    }
}
