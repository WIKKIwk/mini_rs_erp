use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::auth::models::Principal;
use crate::core::qolip::{
    QolipBlock, QolipCellQr, QolipError, QolipLocation, QolipProduct, QolipProductSpec,
    QolipStorePort, role_code,
};

#[derive(Clone)]
pub struct PostgresQolipStore {
    pool: PgPool,
}

impl PostgresQolipStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl QolipStorePort for PostgresQolipStore {
    async fn assigned_warehouses(&self, principal: &Principal) -> Result<Vec<String>, QolipError> {
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT warehouse
            FROM mini_warehouse_assignments
            WHERE principal_ref = $1
              AND lower(principal_role) = lower($2)
              AND btrim(warehouse) <> ''
            ORDER BY lower(warehouse)
            "#,
        )
        .bind(principal.ref_.trim())
        .bind(role_code(&principal.role))
        .fetch_all(&self.pool)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        Ok(rows)
    }

    async fn assigned_blocks(&self, principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        let rows = sqlx::query_as::<_, QolipBlockRow>(
            r#"
            WITH assigned AS (
                SELECT warehouse
                FROM mini_warehouse_assignments
                WHERE principal_ref = $1
                  AND lower(principal_role) = lower($2)
            ),
            child_blocks AS (
                SELECT child.name AS block, assigned.warehouse AS warehouse
                FROM assigned
                JOIN mini_warehouses child
                  ON lower(child.parent_warehouse) = lower(assigned.warehouse)
            )
            SELECT block, warehouse
            FROM child_blocks
            ORDER BY lower(block)
            "#,
        )
        .bind(principal.ref_.trim())
        .bind(role_code(&principal.role))
        .fetch_all(&self.pool)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        Ok(rows
            .into_iter()
            .map(|row| QolipBlock {
                name: row.block,
                warehouse: row.warehouse,
            })
            .collect())
    }

    async fn products(
        &self,
        query: &str,
        limit: usize,
        with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        let query = query.trim().to_lowercase();
        let pattern = format!("%{query}%");
        let rows = sqlx::query_as::<_, QolipProductRow>(
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
                    bool_or(node_name LIKE '%tayyor%' AND node_name LIKE '%mahsulot%') AS is_finished
                FROM group_path
                GROUP BY group_name
            )
            SELECT
                items.code,
                items.name,
                items.item_group,
                COALESCE(spec.qolip_code, '') AS qolip_code,
                COALESCE(spec.size, 0) AS size,
                spec.item_code IS NOT NULL AS has_qolip_spec
            FROM mini_items items
            LEFT JOIN group_kind ON lower(items.item_group) = group_kind.group_name
            LEFT JOIN mini_qolip_product_specs spec
              ON lower(spec.item_code) = lower(items.code)
            WHERE COALESCE(group_kind.is_finished, false)
              AND (NOT $4 OR spec.item_code IS NOT NULL)
              AND (
                $1 = ''
                OR lower(items.code) LIKE $2
                OR lower(items.name) LIKE $2
                OR lower(COALESCE(spec.qolip_code, '')) LIKE $2
              )
            ORDER BY lower(items.name), lower(items.code)
            LIMIT $3
            "#,
        )
        .bind(query)
        .bind(pattern)
        .bind(limit.max(1) as i64)
        .bind(with_qolip_only)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        Ok(rows
            .into_iter()
            .map(|row| QolipProduct {
                code: row.code,
                name: row.name,
                item_group: row.item_group,
                qolip_code: row.qolip_code,
                size: row.size,
                has_qolip_spec: row.has_qolip_spec,
            })
            .collect())
    }

    async fn product_spec(&self, item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        let row = sqlx::query_as::<_, QolipProductSpecRow>(
            "SELECT item_code, item_name, item_group, qolip_code, size,
                    created_by_role, created_by_ref, created_by_name
             FROM mini_qolip_product_specs
             WHERE lower(item_code) = lower($1)",
        )
        .bind(item_code.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        Ok(row.map(row_to_product_spec))
    }

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        let row = sqlx::query_as::<_, QolipProductSpecRow>(
            "INSERT INTO mini_qolip_product_specs (
                 item_code, item_name, item_group, qolip_code, size,
                 created_by_role, created_by_ref, created_by_name, payload_json
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (item_code) DO UPDATE SET
                 item_name = excluded.item_name,
                 item_group = excluded.item_group,
                 qolip_code = excluded.qolip_code,
                 size = excluded.size,
                 created_by_role = excluded.created_by_role,
                 created_by_ref = excluded.created_by_ref,
                 created_by_name = excluded.created_by_name,
                 payload_json = excluded.payload_json,
                 updated_at = now()
             RETURNING item_code, item_name, item_group, qolip_code, size,
                 created_by_role, created_by_ref, created_by_name",
        )
        .bind(spec.item_code.trim())
        .bind(spec.item_name.trim())
        .bind(spec.item_group.trim())
        .bind(spec.qolip_code.trim())
        .bind(spec.size)
        .bind(spec.created_by_role.trim())
        .bind(spec.created_by_ref.trim())
        .bind(spec.created_by_name.trim())
        .bind(serde_json::to_value(&spec).map_err(|_| QolipError::StoreFailed)?)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        Ok(row_to_product_spec(row))
    }

    async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        let rows = sqlx::query_as::<_, QolipLocationRow>(
            "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                    size, quantity, row_letter, column_number, location_label,
                    created_by_role, created_by_ref, created_by_name
             FROM mini_qolip_locations
             WHERE lower(block) = lower($1)
             ORDER BY lower(row_letter), column_number NULLS LAST, lower(item_name), lower(qolip_code)",
        )
        .bind(block.trim())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        Ok(rows.into_iter().map(row_to_location).collect())
    }

    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError> {
        let row = sqlx::query_as::<_, QolipLocationRow>(
            "INSERT INTO mini_qolip_locations (
                 id, block, warehouse, item_code, item_name, qolip_code,
                 size, quantity, row_letter, column_number, location_label,
                 created_by_role, created_by_ref, created_by_name, payload_json
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
             ON CONFLICT (id) DO UPDATE SET
                 block = excluded.block,
                 warehouse = excluded.warehouse,
                 item_code = excluded.item_code,
                 item_name = excluded.item_name,
                 qolip_code = excluded.qolip_code,
                 size = excluded.size,
                 quantity = excluded.quantity,
                 row_letter = excluded.row_letter,
                 column_number = excluded.column_number,
                 location_label = excluded.location_label,
                 created_by_role = excluded.created_by_role,
                 created_by_ref = excluded.created_by_ref,
                 created_by_name = excluded.created_by_name,
                 payload_json = excluded.payload_json,
                 updated_at = now()
             RETURNING id, block, warehouse, item_code, item_name, qolip_code,
                 size, quantity, row_letter, column_number, location_label,
                 created_by_role, created_by_ref, created_by_name",
        )
        .bind(location.id.trim())
        .bind(location.block.trim())
        .bind(location.warehouse.trim())
        .bind(location.item_code.trim())
        .bind(location.item_name.trim())
        .bind(location.qolip_code.trim())
        .bind(location.size)
        .bind(location.quantity)
        .bind(location.row_letter.trim())
        .bind(location.column_number)
        .bind(location.location_label.trim())
        .bind(location.created_by_role.trim())
        .bind(location.created_by_ref.trim())
        .bind(location.created_by_name.trim())
        .bind(serde_json::to_value(&location).map_err(|_| QolipError::StoreFailed)?)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        Ok(row_to_location(row))
    }

    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError> {
        let row = sqlx::query_as::<_, QolipCellQrRow>(
            "INSERT INTO mini_qolip_cell_qrs (
                 id, block, warehouse, row_letter, column_number, location_label,
                 qr_payload, created_by_role, created_by_ref, created_by_name, payload_json
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO UPDATE SET
                 block = excluded.block,
                 warehouse = excluded.warehouse,
                 row_letter = excluded.row_letter,
                 column_number = excluded.column_number,
                 location_label = excluded.location_label,
                 updated_at = now()
             RETURNING id, block, warehouse, row_letter, column_number, location_label,
                 qr_payload, created_by_role, created_by_ref, created_by_name",
        )
        .bind(cell.id.trim())
        .bind(cell.block.trim())
        .bind(cell.warehouse.trim())
        .bind(cell.row_letter.trim())
        .bind(cell.column_number)
        .bind(cell.location_label.trim())
        .bind(cell.qr_payload.trim())
        .bind(cell.created_by_role.trim())
        .bind(cell.created_by_ref.trim())
        .bind(cell.created_by_name.trim())
        .bind(serde_json::to_value(&cell).map_err(|_| QolipError::StoreFailed)?)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        Ok(row_to_cell_qr(row))
    }
}

#[derive(sqlx::FromRow)]
struct QolipBlockRow {
    block: String,
    warehouse: String,
}

#[derive(sqlx::FromRow)]
struct QolipProductRow {
    code: String,
    name: String,
    item_group: String,
    qolip_code: String,
    size: i32,
    has_qolip_spec: bool,
}

#[derive(sqlx::FromRow)]
struct QolipProductSpecRow {
    item_code: String,
    item_name: String,
    item_group: String,
    qolip_code: String,
    size: i32,
    created_by_role: String,
    created_by_ref: String,
    created_by_name: String,
}

#[derive(sqlx::FromRow)]
struct QolipLocationRow {
    id: String,
    block: String,
    warehouse: String,
    item_code: String,
    item_name: String,
    qolip_code: String,
    size: i32,
    quantity: i32,
    row_letter: String,
    column_number: Option<i32>,
    location_label: String,
    created_by_role: String,
    created_by_ref: String,
    created_by_name: String,
}

#[derive(sqlx::FromRow)]
struct QolipCellQrRow {
    id: String,
    block: String,
    warehouse: String,
    row_letter: String,
    column_number: i32,
    location_label: String,
    qr_payload: String,
    created_by_role: String,
    created_by_ref: String,
    created_by_name: String,
}

fn row_to_location(row: QolipLocationRow) -> QolipLocation {
    QolipLocation {
        id: row.id,
        block: row.block,
        warehouse: row.warehouse,
        item_code: row.item_code,
        item_name: row.item_name,
        qolip_code: row.qolip_code,
        size: row.size,
        quantity: row.quantity,
        row_letter: row.row_letter,
        column_number: row.column_number,
        location_label: row.location_label,
        created_by_role: row.created_by_role,
        created_by_ref: row.created_by_ref,
        created_by_name: row.created_by_name,
    }
}

fn row_to_product_spec(row: QolipProductSpecRow) -> QolipProductSpec {
    QolipProductSpec {
        item_code: row.item_code,
        item_name: row.item_name,
        item_group: row.item_group,
        qolip_code: row.qolip_code,
        size: row.size,
        created_by_role: row.created_by_role,
        created_by_ref: row.created_by_ref,
        created_by_name: row.created_by_name,
    }
}

fn row_to_cell_qr(row: QolipCellQrRow) -> QolipCellQr {
    QolipCellQr {
        id: row.id,
        block: row.block,
        warehouse: row.warehouse,
        row_letter: row.row_letter,
        column_number: row.column_number,
        location_label: row.location_label,
        qr_payload: row.qr_payload,
        created_by_role: row.created_by_role,
        created_by_ref: row.created_by_ref,
        created_by_name: row.created_by_name,
    }
}
