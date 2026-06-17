use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::admin::models::AdminWarehouse;
use crate::core::auth::models::PrincipalRole;
use crate::core::warehouses::{
    WarehouseAssignment, WarehouseError, WarehouseStorePort, WarehouseSummary,
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
            WITH RECURSIVE group_path(group_name, node_name, parent_name) AS (
                SELECT lower(name), lower(name), lower(parent_item_group)
                FROM mini_item_groups
                UNION ALL
                SELECT group_path.group_name, lower(parent.name), lower(parent.parent_item_group)
                FROM group_path
                JOIN mini_item_groups parent ON lower(parent.name) = group_path.parent_name
                WHERE group_path.parent_name <> ''
            ),
            group_kind AS (
                SELECT
                    group_name,
                    bool_or(node_name LIKE '%homashyo%' OR node_name LIKE '%xomashyo%') AS is_raw,
                    bool_or(node_name LIKE '%tayyor%' AND node_name LIKE '%mahsulot%') AS is_finished
                FROM group_path
                GROUP BY group_name
            ),
            warehouse_kind AS (
                SELECT
                    (
                        SELECT name
                        FROM mini_warehouses
                        WHERE lower(name) LIKE '%homashyo%' OR lower(name) LIKE '%xomashyo%'
                        ORDER BY lower(name)
                        LIMIT 1
                    ) AS raw_warehouse,
                    (
                        SELECT name
                        FROM mini_warehouses
                        WHERE lower(name) LIKE '%tayyor%' AND lower(name) LIKE '%mahsulot%'
                        ORDER BY lower(name)
                        LIMIT 1
                    ) AS finished_warehouse
            ),
            item_map AS (
                SELECT
                    items.code,
                    COALESCE(
                        NULLIF(btrim(items.warehouse), ''),
                        CASE
                            WHEN group_kind.is_raw THEN warehouse_kind.raw_warehouse
                            WHEN group_kind.is_finished THEN warehouse_kind.finished_warehouse
                            ELSE ''
                        END,
                        ''
                    ) AS warehouse
                FROM mini_items items
                LEFT JOIN group_kind ON lower(items.item_group) = group_kind.group_name
                CROSS JOIN warehouse_kind
            ),
            item_counts AS (
                SELECT warehouse, count(*)::bigint AS item_count
                FROM item_map
                WHERE btrim(warehouse) <> ''
                GROUP BY warehouse
            ),
            raw_counts AS (
                SELECT warehouse, count(*)::bigint AS raw_count
                FROM mini_raw_material_stock
                GROUP BY warehouse
            ),
            reservation_counts AS (
                SELECT
                    COALESCE(NULLIF(btrim(stock.warehouse), ''), NULLIF(btrim(item_map.warehouse), '')) AS warehouse,
                    count(*)::bigint AS reserved_count
                FROM mini_raw_material_assignments assignments
                LEFT JOIN mini_raw_material_stock stock
                    ON lower(stock.barcode) = lower(assignments.barcode)
                LEFT JOIN item_map
                    ON lower(item_map.code) = lower(assignments.item_code)
                GROUP BY COALESCE(NULLIF(btrim(stock.warehouse), ''), NULLIF(btrim(item_map.warehouse), ''))
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
                SELECT warehouse FROM item_counts
                UNION
                SELECT warehouse FROM raw_counts
                UNION
                SELECT warehouse FROM reservation_counts WHERE btrim(COALESCE(warehouse, '')) <> ''
                UNION
                SELECT warehouse FROM assignment_counts
            )
            SELECT
                warehouse_names.warehouse,
                (COALESCE(item_counts.item_count, 0) + COALESCE(raw_counts.raw_count, 0))::bigint AS product_count,
                COALESCE(reservation_counts.reserved_count, 0)::bigint AS reserved_count,
                COALESCE(assignment_counts.assignment_count, 0)::bigint AS assignment_count,
                COALESCE(assignment_counts.assigned_display_names, '') AS assigned_display_names
            FROM warehouse_names
            LEFT JOIN item_counts ON lower(item_counts.warehouse) = lower(warehouse_names.warehouse)
            LEFT JOIN raw_counts ON lower(raw_counts.warehouse) = lower(warehouse_names.warehouse)
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

fn warehouse_id(name: &str) -> String {
    format!("warehouse:{}", name.trim().to_lowercase())
}

fn role_as_str(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Admin => "admin",
    }
}

fn role_from_str(raw: &str) -> Result<PrincipalRole, WarehouseError> {
    match raw.trim().to_lowercase().as_str() {
        "supplier" => Ok(PrincipalRole::Supplier),
        "werka" => Ok(PrincipalRole::Werka),
        "customer" => Ok(PrincipalRole::Customer),
        "aparatchi" => Ok(PrincipalRole::Aparatchi),
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
