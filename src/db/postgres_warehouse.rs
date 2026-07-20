use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::admin::models::AdminWarehouse;
use crate::core::auth::models::PrincipalRole;
use crate::core::warehouses::{
    WarehouseAssignment, WarehouseError, WarehouseStockItem, WarehouseStorePort, WarehouseSummary,
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
            "SELECT warehouse, principal_role, principal_ref, display_name
             FROM mini_warehouse_assignments
             WHERE $1 = '' OR lower(warehouse) = $1
             ORDER BY lower(warehouse), lower(display_name), lower(principal_ref)",
        )
        .bind(warehouse)
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
                SELECT warehouse, count(*)::bigint AS raw_count
                FROM mini_raw_material_stock
                WHERE status = 'available' AND qty > 0
                GROUP BY warehouse
            ),
            finished_counts AS (
                SELECT
                    warehouse,
                    count(DISTINCT (lower(item_code), lower(uom)))::bigint AS finished_count
                FROM mini_finished_goods_stock
                WHERE status = 'available' AND qty > 0
                GROUP BY warehouse
            ),
            qolip_counts AS (
                SELECT warehouse, COALESCE(sum(quantity), 0)::bigint AS qolip_count
                FROM mini_qolip_locations
                WHERE btrim(warehouse) <> ''
                GROUP BY warehouse
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
            assignment_counts AS (
                SELECT
                    warehouse,
                    count(*)::bigint AS assignment_count,
                    string_agg(COALESCE(NULLIF(btrim(display_name), ''), principal_ref), E'\n'
                        ORDER BY lower(COALESCE(NULLIF(btrim(display_name), ''), principal_ref))) AS assigned_display_names
                FROM mini_warehouse_assignments
                GROUP BY warehouse
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
                )::bigint AS reserved_count,
                COALESCE(assignment_counts.assignment_count, 0)::bigint AS assignment_count,
                COALESCE(assignment_counts.assigned_display_names, '') AS assigned_display_names
            FROM warehouse_names
            LEFT JOIN raw_counts ON lower(raw_counts.warehouse) = lower(warehouse_names.warehouse)
            LEFT JOIN finished_counts ON lower(finished_counts.warehouse) = lower(warehouse_names.warehouse)
            LEFT JOIN qolip_counts ON lower(qolip_counts.warehouse) = lower(warehouse_names.warehouse)
            LEFT JOIN qolip_checkout_counts ON lower(qolip_checkout_counts.warehouse) = lower(warehouse_names.warehouse)
            LEFT JOIN reservation_counts ON lower(reservation_counts.warehouse) = lower(warehouse_names.warehouse)
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
            WHERE lower(stock.warehouse) = lower($1)
              AND stock.status = 'available'
              AND stock.qty > 0
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
        sqlx::query_as::<_, WarehouseAssignmentRow>(
            "INSERT INTO mini_warehouse_assignments (
                 warehouse, principal_role, principal_ref, display_name, payload_json
             )
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (warehouse, principal_role, principal_ref) DO UPDATE SET
               display_name = excluded.display_name,
               payload_json = excluded.payload_json,
               updated_at = now()
             RETURNING warehouse, principal_role, principal_ref, display_name",
        )
        .bind(warehouse)
        .bind(role_as_str(&assignment.principal_role))
        .bind(principal_ref)
        .bind(assignment.display_name.trim())
        .bind(serde_json::json!({
            "warehouse": warehouse,
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
        warehouse: &str,
        principal_role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<Option<WarehouseAssignment>, WarehouseError> {
        let row = sqlx::query_as::<_, WarehouseAssignmentRow>(
            "DELETE FROM mini_warehouse_assignments
             WHERE warehouse = $1
               AND principal_role = $2
               AND principal_ref = $3
             RETURNING warehouse, principal_role, principal_ref, display_name",
        )
        .bind(warehouse.trim())
        .bind(role_as_str(principal_role))
        .bind(principal_ref.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| WarehouseError::StoreFailed)?;

        row.map(row_to_assignment).transpose()
    }

    async fn delete_warehouse(&self, warehouse: &str) -> Result<(), WarehouseError> {
        let warehouse = warehouse.trim();
        if warehouse.is_empty() {
            return Err(WarehouseError::MissingWarehouse);
        }
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        for table in ["mini_qolip_cell_qrs", "mini_qolip_locations"] {
            sqlx::query(&format!(
                "DELETE FROM {table}
                 WHERE lower(warehouse) = lower($1) OR lower(block) = lower($1)"
            ))
            .bind(warehouse)
            .execute(&mut *transaction)
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        }
        for table in ["mini_raw_material_stock", "mini_finished_goods_stock"] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE lower(warehouse) = lower($1)"
            ))
            .bind(warehouse)
            .execute(&mut *transaction)
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        }
        sqlx::query("DELETE FROM mini_warehouse_assignments WHERE lower(warehouse) = lower($1)")
            .bind(warehouse)
            .execute(&mut *transaction)
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        sqlx::query("DELETE FROM mini_warehouses WHERE lower(name) = lower($1)")
            .bind(warehouse)
            .execute(&mut *transaction)
            .await
            .map_err(|_| WarehouseError::StoreFailed)?;
        transaction
            .commit()
            .await
            .map_err(|_| WarehouseError::StoreFailed)
    }
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
    warehouse: String,
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
        warehouse: row.warehouse,
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
