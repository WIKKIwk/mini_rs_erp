use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::auth::models::Principal;
use crate::core::qolip::normalize::{
    location_from_checkout, location_from_checkout_target, location_identity_matches,
    normalize_move_target,
};
use crate::core::qolip::{
    QolipBlock, QolipCellQr, QolipCheckout, QolipError, QolipLocation, QolipProduct,
    QolipProductSpec, QolipStorePort, role_code,
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

    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError> {
        let rows = sqlx::query_as::<_, QolipBlockRow>(
            r#"
            SELECT child.name AS block, child.parent_warehouse AS warehouse
            FROM mini_warehouses child
            WHERE child.is_group = false
              AND btrim(child.parent_warehouse) <> ''
            ORDER BY lower(child.parent_warehouse), lower(child.name)
            "#,
        )
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
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| QolipError::StoreFailed)?;

        let existing_row = sqlx::query_as::<_, QolipLocationRow>(
            "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                    size, quantity, row_letter, column_number, location_label,
                    created_by_role, created_by_ref, created_by_name
             FROM mini_qolip_locations
             WHERE id = $1
             FOR UPDATE",
        )
        .bind(location.id.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        let row = if let Some(existing_row) = existing_row {
            let existing = row_to_location(existing_row);
            if !location_identity_matches(&existing, &location) {
                return Err(QolipError::LocationIdentityMismatch);
            }
            sqlx::query_as::<_, QolipLocationRow>(
                "UPDATE mini_qolip_locations
                 SET block = $2,
                     warehouse = $3,
                     item_code = $4,
                     item_name = $5,
                     qolip_code = $6,
                     size = $7,
                     quantity = quantity + $8,
                     row_letter = $9,
                     column_number = $10,
                     location_label = $11,
                     created_by_role = $12,
                     created_by_ref = $13,
                     created_by_name = $14,
                     payload_json = $15,
                     updated_at = now()
                 WHERE id = $1
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
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?
        } else {
            sqlx::query_as::<_, QolipLocationRow>(
                "INSERT INTO mini_qolip_locations (
                     id, block, warehouse, item_code, item_name, qolip_code,
                     size, quantity, row_letter, column_number, location_label,
                     created_by_role, created_by_ref, created_by_name, payload_json
                 )
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
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
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?
        };

        tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
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

    async fn location_by_id(&self, location_id: &str) -> Result<Option<QolipLocation>, QolipError> {
        let row = sqlx::query_as::<_, QolipLocationRow>(
            "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                    size, quantity, row_letter, column_number, location_label,
                    created_by_role, created_by_ref, created_by_name
             FROM mini_qolip_locations
             WHERE id = $1",
        )
        .bind(location_id.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        Ok(row.map(row_to_location))
    }

    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| QolipError::StoreFailed)?;

        let current_row = sqlx::query_as::<_, QolipLocationRow>(
            "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                    size, quantity, row_letter, column_number, location_label,
                    created_by_role, created_by_ref, created_by_name
             FROM mini_qolip_locations
             WHERE id = $1
             FOR UPDATE",
        )
        .bind(checkout.location_id.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        let Some(current_row) = current_row else {
            return Err(QolipError::LocationNotFound);
        };
        let current = row_to_location(current_row);
        let expected = location_from_checkout(&checkout);
        if !location_identity_matches(&current, &expected) {
            return Err(QolipError::LocationIdentityMismatch);
        }
        let current_qty = current.quantity;
        if checkout.quantity > current_qty {
            return Err(QolipError::InsufficientStock);
        }

        let remaining = current_qty - checkout.quantity;
        if remaining > 0 {
            sqlx::query(
                "UPDATE mini_qolip_locations
                 SET quantity = $2, updated_at = now()
                 WHERE id = $1",
            )
            .bind(checkout.location_id.trim())
            .bind(remaining)
            .execute(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
        } else {
            sqlx::query("DELETE FROM mini_qolip_locations WHERE id = $1")
                .bind(checkout.location_id.trim())
                .execute(&mut *tx)
                .await
                .map_err(|_| QolipError::StoreFailed)?;
        }

        let row = sqlx::query_as::<_, QolipCheckoutRow>(
            "INSERT INTO mini_qolip_checkouts (
                 id, location_id, block, warehouse, item_code, item_name, qolip_code,
                 size, quantity, row_letter, column_number, location_label,
                 issued_to_ref, issued_to_name, status,
                 issued_by_role, issued_by_ref, issued_by_name, payload_json
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
             RETURNING id, location_id, block, warehouse, item_code, item_name, qolip_code,
                 size, quantity, row_letter, column_number, location_label,
                 issued_to_ref, issued_to_name, status,
                 issued_by_role, issued_by_ref, issued_by_name,
                 to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at",
        )
        .bind(checkout.id.trim())
        .bind(checkout.location_id.trim())
        .bind(checkout.block.trim())
        .bind(checkout.warehouse.trim())
        .bind(checkout.item_code.trim())
        .bind(checkout.item_name.trim())
        .bind(checkout.qolip_code.trim())
        .bind(checkout.size)
        .bind(checkout.quantity)
        .bind(checkout.row_letter.trim())
        .bind(checkout.column_number)
        .bind(checkout.location_label.trim())
        .bind(checkout.issued_to_ref.trim())
        .bind(checkout.issued_to_name.trim())
        .bind(checkout.status.trim())
        .bind(checkout.issued_by_role.trim())
        .bind(checkout.issued_by_ref.trim())
        .bind(checkout.issued_by_name.trim())
        .bind(serde_json::to_value(&checkout).map_err(|_| QolipError::StoreFailed)?)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
        Ok(row_to_checkout(row))
    }

    async fn checkouts(
        &self,
        block: Option<&str>,
        allowed_blocks: Option<&[String]>,
        status: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        let block = block.map(str::trim).filter(|value| !value.is_empty());
        let rows = if let Some(block) = block {
            sqlx::query_as::<_, QolipCheckoutRow>(
                "SELECT id, location_id, block, warehouse, item_code, item_name, qolip_code,
                        size, quantity, row_letter, column_number, location_label,
                        issued_to_ref, issued_to_name, status,
                        issued_by_role, issued_by_ref, issued_by_name,
                        to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at
                 FROM mini_qolip_checkouts
                 WHERE lower(status) = lower($1)
                   AND lower(block) = lower($2)
                 ORDER BY issued_at DESC
                 LIMIT $3",
            )
            .bind(status.trim())
            .bind(block)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
        } else if let Some(allowed_blocks) = allowed_blocks {
            if allowed_blocks.is_empty() {
                return Ok(Vec::new());
            }
            let allowed: Vec<String> = allowed_blocks
                .iter()
                .map(|block| block.trim().to_lowercase())
                .filter(|block| !block.is_empty())
                .collect();
            if allowed.is_empty() {
                return Ok(Vec::new());
            }
            sqlx::query_as::<_, QolipCheckoutRow>(
                "SELECT id, location_id, block, warehouse, item_code, item_name, qolip_code,
                        size, quantity, row_letter, column_number, location_label,
                        issued_to_ref, issued_to_name, status,
                        issued_by_role, issued_by_ref, issued_by_name,
                        to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at
                 FROM mini_qolip_checkouts
                 WHERE lower(status) = lower($1)
                   AND lower(block) = ANY($2)
                 ORDER BY issued_at DESC
                 LIMIT $3",
            )
            .bind(status.trim())
            .bind(allowed)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as::<_, QolipCheckoutRow>(
                "SELECT id, location_id, block, warehouse, item_code, item_name, qolip_code,
                        size, quantity, row_letter, column_number, location_label,
                        issued_to_ref, issued_to_name, status,
                        issued_by_role, issued_by_ref, issued_by_name,
                        to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at
                 FROM mini_qolip_checkouts
                 WHERE lower(status) = lower($1)
                 ORDER BY issued_at DESC
                 LIMIT $2",
            )
            .bind(status.trim())
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
        }
        .map_err(|_| QolipError::StoreFailed)?;

        Ok(rows.into_iter().map(row_to_checkout).collect())
    }

    async fn checkout_by_id(&self, checkout_id: &str) -> Result<Option<QolipCheckout>, QolipError> {
        let row = sqlx::query_as::<_, QolipCheckoutRow>(
            "SELECT id, location_id, block, warehouse, item_code, item_name, qolip_code,
                    size, quantity, row_letter, column_number, location_label,
                    issued_to_ref, issued_to_name, status,
                    issued_by_role, issued_by_ref, issued_by_name,
                    to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at
             FROM mini_qolip_checkouts
             WHERE id = $1",
        )
        .bind(checkout_id.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        Ok(row.map(row_to_checkout))
    }

    async fn return_checkout(
        &self,
        checkout_id: &str,
        row_letter: &str,
        column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError> {
        let checkout_id = checkout_id.trim();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| QolipError::StoreFailed)?;

        let row = sqlx::query_as::<_, QolipCheckoutRow>(
            "UPDATE mini_qolip_checkouts
             SET status = 'returned', updated_at = now()
             WHERE id = $1 AND lower(status) = 'open'
             RETURNING id, location_id, block, warehouse, item_code, item_name, qolip_code,
                 size, quantity, row_letter, column_number, location_label,
                 issued_to_ref, issued_to_name, status,
                 issued_by_role, issued_by_ref, issued_by_name,
                 to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at",
        )
        .bind(checkout_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        let Some(row) = row else {
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM mini_qolip_checkouts WHERE id = $1)",
            )
            .bind(checkout_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
            return Err(if exists {
                QolipError::CheckoutNotReturnable
            } else {
                QolipError::CheckoutNotFound
            });
        };

        let checkout = row_to_checkout(row);
        let restore = location_from_checkout_target(&checkout, row_letter, column_number)?;
        let existing_row = sqlx::query_as::<_, QolipLocationRow>(
            "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                    size, quantity, row_letter, column_number, location_label,
                    created_by_role, created_by_ref, created_by_name
             FROM mini_qolip_locations
             WHERE id = $1
             FOR UPDATE",
        )
        .bind(restore.id.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        if let Some(existing_row) = existing_row {
            let existing = row_to_location(existing_row);
            if !location_identity_matches(&existing, &restore) {
                return Err(QolipError::LocationIdentityMismatch);
            }
            sqlx::query(
                "UPDATE mini_qolip_locations
                 SET quantity = $2, updated_at = now()
                 WHERE id = $1",
            )
            .bind(restore.id.trim())
            .bind(existing.quantity + restore.quantity)
            .execute(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
        } else {
            sqlx::query(
                "INSERT INTO mini_qolip_locations (
                     id, block, warehouse, item_code, item_name, qolip_code,
                     size, quantity, row_letter, column_number, location_label,
                     created_by_role, created_by_ref, created_by_name, payload_json
                 )
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
            )
            .bind(restore.id.trim())
            .bind(restore.block.trim())
            .bind(restore.warehouse.trim())
            .bind(restore.item_code.trim())
            .bind(restore.item_name.trim())
            .bind(restore.qolip_code.trim())
            .bind(restore.size)
            .bind(restore.quantity)
            .bind(restore.row_letter.trim())
            .bind(restore.column_number)
            .bind(restore.location_label.trim())
            .bind(restore.created_by_role.trim())
            .bind(restore.created_by_ref.trim())
            .bind(restore.created_by_name.trim())
            .bind(serde_json::to_value(&restore).map_err(|_| QolipError::StoreFailed)?)
            .execute(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
        }

        tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
        Ok(checkout)
    }

    async fn move_location(
        &self,
        location_id: &str,
        row_letter: &str,
        column_number: i32,
        quantity: i32,
    ) -> Result<QolipLocation, QolipError> {
        let location_id = location_id.trim();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| QolipError::StoreFailed)?;

        let source_row = sqlx::query_as::<_, QolipLocationRow>(
            "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                    size, quantity, row_letter, column_number, location_label,
                    created_by_role, created_by_ref, created_by_name
             FROM mini_qolip_locations
             WHERE id = $1",
        )
        .bind(location_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        let Some(source_row) = source_row else {
            return Err(QolipError::LocationNotFound);
        };
        let source = row_to_location(source_row);
        let target = normalize_move_target(&source, row_letter, column_number, quantity)?;

        let mut lock_ids = vec![source.id.clone(), target.id.clone()];
        lock_ids.sort();
        lock_ids.dedup();
        for lock_id in &lock_ids {
            sqlx::query("SELECT id FROM mini_qolip_locations WHERE id = $1 FOR UPDATE")
                .bind(lock_id.trim())
                .fetch_optional(&mut *tx)
                .await
                .map_err(|_| QolipError::StoreFailed)?;
        }

        let source_row = sqlx::query_as::<_, QolipLocationRow>(
            "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                    size, quantity, row_letter, column_number, location_label,
                    created_by_role, created_by_ref, created_by_name
             FROM mini_qolip_locations
             WHERE id = $1",
        )
        .bind(location_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
        let Some(source_row) = source_row else {
            return Err(QolipError::LocationNotFound);
        };
        let source = row_to_location(source_row);
        let target = normalize_move_target(&source, row_letter, column_number, quantity)?;

        let target_row = sqlx::query_as::<_, QolipLocationRow>(
            "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                    size, quantity, row_letter, column_number, location_label,
                    created_by_role, created_by_ref, created_by_name
             FROM mini_qolip_locations
             WHERE id = $1",
        )
        .bind(target.id.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
        if let Some(existing_row) = &target_row {
            let existing = row_to_location(existing_row.clone());
            if !location_identity_matches(&existing, &target) {
                return Err(QolipError::LocationIdentityMismatch);
            }
        }

        let remaining = source.quantity - quantity;
        if remaining > 0 {
            sqlx::query(
                "UPDATE mini_qolip_locations
                 SET quantity = $2, updated_at = now()
                 WHERE id = $1",
            )
            .bind(source.id.trim())
            .bind(remaining)
            .execute(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
        } else {
            sqlx::query("DELETE FROM mini_qolip_locations WHERE id = $1")
                .bind(source.id.trim())
                .execute(&mut *tx)
                .await
                .map_err(|_| QolipError::StoreFailed)?;
        }

        let saved = if let Some(existing_row) = target_row {
            let merged_qty = existing_row.quantity + target.quantity;
            let row = sqlx::query_as::<_, QolipLocationRow>(
                "UPDATE mini_qolip_locations
                 SET quantity = $2, updated_at = now()
                 WHERE id = $1
                 RETURNING id, block, warehouse, item_code, item_name, qolip_code,
                     size, quantity, row_letter, column_number, location_label,
                     created_by_role, created_by_ref, created_by_name",
            )
            .bind(target.id.trim())
            .bind(merged_qty)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
            row_to_location(row)
        } else {
            let row = sqlx::query_as::<_, QolipLocationRow>(
                "INSERT INTO mini_qolip_locations (
                     id, block, warehouse, item_code, item_name, qolip_code,
                     size, quantity, row_letter, column_number, location_label,
                     created_by_role, created_by_ref, created_by_name, payload_json
                 )
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
                 RETURNING id, block, warehouse, item_code, item_name, qolip_code,
                     size, quantity, row_letter, column_number, location_label,
                     created_by_role, created_by_ref, created_by_name",
            )
            .bind(target.id.trim())
            .bind(target.block.trim())
            .bind(target.warehouse.trim())
            .bind(target.item_code.trim())
            .bind(target.item_name.trim())
            .bind(target.qolip_code.trim())
            .bind(target.size)
            .bind(target.quantity)
            .bind(target.row_letter.trim())
            .bind(target.column_number)
            .bind(target.location_label.trim())
            .bind(target.created_by_role.trim())
            .bind(target.created_by_ref.trim())
            .bind(target.created_by_name.trim())
            .bind(serde_json::to_value(&target).map_err(|_| QolipError::StoreFailed)?)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
            row_to_location(row)
        };

        tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
        Ok(saved)
    }

    async fn cell_qr_by_payload(
        &self,
        qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        let row = sqlx::query_as::<_, QolipCellQrRow>(
            "SELECT id, block, warehouse, row_letter, column_number, location_label,
                    qr_payload, created_by_role, created_by_ref, created_by_name
             FROM mini_qolip_cell_qrs
             WHERE lower(qr_payload) = lower($1)",
        )
        .bind(qr_payload.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

        Ok(row.map(row_to_cell_qr))
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

#[derive(Clone, sqlx::FromRow)]
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

#[derive(sqlx::FromRow)]
struct QolipCheckoutRow {
    id: String,
    location_id: String,
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
    issued_to_ref: String,
    issued_to_name: String,
    status: String,
    issued_by_role: String,
    issued_by_ref: String,
    issued_by_name: String,
    issued_at: String,
}

fn row_to_checkout(row: QolipCheckoutRow) -> QolipCheckout {
    QolipCheckout {
        id: row.id,
        location_id: row.location_id,
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
        issued_to_ref: row.issued_to_ref,
        issued_to_name: row.issued_to_name,
        status: row.status,
        issued_by_role: row.issued_by_role,
        issued_by_ref: row.issued_by_ref,
        issued_by_name: row.issued_by_name,
        issued_at: row.issued_at,
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
