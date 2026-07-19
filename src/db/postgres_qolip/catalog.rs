use sqlx::PgPool;

use crate::core::auth::models::Principal;
use crate::core::qolip::{QolipBlock, QolipError, QolipProduct, QolipProductSpec, role_code};

use super::rows::{QolipBlockRow, QolipProductRow, QolipProductSpecRow, row_to_product_spec};

pub(super) async fn load_assigned_warehouses(
    pool: &PgPool,
    principal: &Principal,
) -> Result<Vec<String>, QolipError> {
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
    .fetch_all(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(rows)
}

pub(super) async fn load_assigned_blocks(
    pool: &PgPool,
    principal: &Principal,
) -> Result<Vec<QolipBlock>, QolipError> {
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
    .fetch_all(pool)
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

pub(super) async fn load_all_blocks(pool: &PgPool) -> Result<Vec<QolipBlock>, QolipError> {
    let rows = sqlx::query_as::<_, QolipBlockRow>(
        r#"
        SELECT child.name AS block, child.parent_warehouse AS warehouse
        FROM mini_warehouses child
        WHERE child.is_group = false
          AND btrim(child.parent_warehouse) <> ''
        ORDER BY lower(child.parent_warehouse), lower(child.name)
        "#,
    )
    .fetch_all(pool)
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

pub(super) async fn load_products(
    pool: &PgPool,
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
            spec.item_code IS NOT NULL AS has_qolip_spec,
            EXISTS (
                SELECT 1
                FROM mini_qolip_checkouts checkout
                WHERE lower(checkout.qolip_code) = lower(spec.qolip_code)
                  AND lower(checkout.status) = 'open'
            ) AS is_in_use
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
    .fetch_all(pool)
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
            is_in_use: row.is_in_use,
        })
        .collect())
}

pub(super) async fn load_product_spec(
    pool: &PgPool,
    item_code: &str,
) -> Result<Option<QolipProductSpec>, QolipError> {
    let row = sqlx::query_as::<_, QolipProductSpecRow>(
        "SELECT item_code, item_name, item_group, qolip_code, size,
                created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_product_specs
         WHERE lower(item_code) = lower($1)
         ORDER BY updated_at DESC, created_at DESC
         LIMIT 1",
    )
    .bind(item_code.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(row.map(row_to_product_spec))
}

pub(super) async fn load_product_spec_by_qolip_code(
    pool: &PgPool,
    qolip_code: &str,
) -> Result<Option<QolipProductSpec>, QolipError> {
    let row = sqlx::query_as::<_, QolipProductSpecRow>(
        "SELECT item_code, item_name, item_group, qolip_code, size,
                created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_product_specs
         WHERE lower(qolip_code) = lower($1)",
    )
    .bind(qolip_code.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(row.map(row_to_product_spec))
}

pub(super) async fn save_product_spec(
    pool: &PgPool,
    spec: QolipProductSpec,
) -> Result<QolipProductSpec, QolipError> {
    let row = sqlx::query_as::<_, QolipProductSpecRow>(
        "INSERT INTO mini_qolip_product_specs (
             item_code, item_name, item_group, qolip_code, size,
             created_by_role, created_by_ref, created_by_name, payload_json
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT (lower(qolip_code)) DO UPDATE SET
             item_name = excluded.item_name,
             item_group = excluded.item_group,
             item_code = excluded.item_code,
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
    .fetch_one(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(row_to_product_spec(row))
}

pub(super) async fn delete_product_specs(
    pool: &PgPool,
    qolip_codes: &[String],
) -> Result<usize, QolipError> {
    let mut normalized = qolip_codes
        .iter()
        .map(|code| code.trim().to_lowercase())
        .filter(|code| !code.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    if normalized.is_empty() {
        return Err(QolipError::MissingQolipCode);
    }

    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;
    for code in &normalized {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1)::bigint)")
            .bind(code)
            .execute(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
    }

    let locked_codes = sqlx::query_scalar::<_, String>(
        "SELECT qolip_code
         FROM mini_qolip_product_specs
         WHERE lower(qolip_code) = ANY($1)
         ORDER BY lower(qolip_code)
         FOR UPDATE",
    )
    .bind(&normalized)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    let in_use = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM mini_qolip_checkouts
             WHERE lower(qolip_code) = ANY($1)
               AND lower(status) = 'open'
         )",
    )
    .bind(&normalized)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    if in_use {
        return Err(QolipError::QolipInUse);
    }

    sqlx::query("DELETE FROM mini_qolip_locations WHERE lower(qolip_code) = ANY($1)")
        .bind(&normalized)
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    let deleted =
        sqlx::query("DELETE FROM mini_qolip_product_specs WHERE lower(qolip_code) = ANY($1)")
            .bind(&normalized)
            .execute(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?
            .rows_affected() as usize;
    debug_assert!(deleted <= locked_codes.len());
    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(deleted)
}
