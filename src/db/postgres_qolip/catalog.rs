use sqlx::{PgPool, Postgres, Transaction};

use crate::core::auth::models::Principal;
use crate::core::qolip::normalize::qolip_location_id;
use crate::core::qolip::{role_code, QolipBlock, QolipError, QolipProduct, QolipProductSpec};

use super::rows::{row_to_product_spec, QolipBlockRow, QolipProductRow, QolipProductSpecRow};

const QOLIP_PANTON_MAX_NUMBER: i32 = 100;

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

pub(super) async fn rename_block(
    pool: &PgPool,
    block: &str,
    new_block: &str,
    warehouse: &str,
) -> Result<QolipBlock, QolipError> {
    let block = block.trim();
    let new_block = new_block.trim();
    let warehouse = warehouse.trim();
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;
    let mut lock_keys = [block.to_lowercase(), new_block.to_lowercase()];
    lock_keys.sort();
    for key in lock_keys {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1)::bigint)")
            .bind(key)
            .execute(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
    }

    let current = sqlx::query_as::<_, QolipBlockRow>(
        "SELECT name AS block, parent_warehouse AS warehouse
         FROM mini_warehouses
         WHERE lower(name) = lower($1)
           AND lower(parent_warehouse) = lower($2)
         FOR UPDATE",
    )
    .bind(block)
    .bind(warehouse)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?
    .ok_or(QolipError::MissingBlock)?;

    let conflict = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM mini_warehouses
             WHERE lower(name) = lower($1)
               AND lower(name) <> lower($2)
         )",
    )
    .bind(new_block)
    .bind(block)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    if conflict {
        return Err(QolipError::StoreFailed);
    }

    let renamed = sqlx::query_as::<_, QolipBlockRow>(
        "UPDATE mini_warehouses
         SET name = $2,
             payload_json = jsonb_set(
                 COALESCE(payload_json, '{}'::jsonb),
                 '{warehouse}',
                 to_jsonb($2::text),
                 true
             ),
             updated_at = now()
         WHERE lower(name) = lower($1)
         RETURNING name AS block, parent_warehouse AS warehouse",
    )
    .bind(&current.block)
    .bind(new_block)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    sqlx::query(
        "UPDATE mini_warehouses
         SET parent_warehouse = $2,
             payload_json = jsonb_set(
                 COALESCE(payload_json, '{}'::jsonb),
                 '{parent_warehouse}',
                 to_jsonb($2::text),
                 true
             ),
             updated_at = now()
         WHERE lower(parent_warehouse) = lower($1)",
    )
    .bind(&current.block)
    .bind(new_block)
    .execute(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    sqlx::query(
        "UPDATE mini_warehouse_assignments
         SET warehouse = $2,
             payload_json = jsonb_set(
                 COALESCE(payload_json, '{}'::jsonb),
                 '{warehouse}',
                 to_jsonb($2::text),
                 true
             ),
             updated_at = now()
         WHERE lower(warehouse) = lower($1)",
    )
    .bind(&current.block)
    .bind(new_block)
    .execute(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    for table in [
        "mini_qolip_locations",
        "mini_qolip_cell_qrs",
        "mini_qolip_checkouts",
    ] {
        sqlx::query(&format!(
            "UPDATE {table}
             SET block = $2,
                 payload_json = jsonb_set(
                     COALESCE(payload_json, '{{}}'::jsonb),
                     '{{block}}',
                     to_jsonb($2::text),
                     true
                 ),
                 updated_at = now()
             WHERE lower(block) = lower($1)"
        ))
        .bind(&current.block)
        .bind(new_block)
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    }

    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(QolipBlock {
        name: renamed.block,
        warehouse: renamed.warehouse,
    })
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
        ),
        eligible_items AS (
            SELECT items.code, items.name, items.item_group, items.payload_json
            FROM mini_items items
            LEFT JOIN group_kind ON lower(items.item_group) = group_kind.group_name
            WHERE COALESCE(group_kind.is_finished, false)
        ),
        legacy_locations AS (
            SELECT DISTINCT ON (lower(location.qolip_code))
                location.item_code,
                location.item_name,
                COALESCE(NULLIF(btrim(items.item_group), ''), '') AS item_group,
                location.qolip_code,
                location.size,
                COALESCE(location.payload_json->>'color', '') AS color,
                location.created_at,
                location.updated_at
            FROM mini_qolip_locations location
            LEFT JOIN mini_items items
              ON lower(items.code) = lower(location.item_code)
            WHERE NOT EXISTS (
                SELECT 1
                FROM mini_qolip_product_specs spec
                WHERE lower(spec.qolip_code) = lower(location.qolip_code)
            )
            ORDER BY lower(location.qolip_code), location.updated_at DESC, location.created_at DESC
        ),
        legacy_checkouts AS (
            SELECT DISTINCT ON (lower(checkout.qolip_code))
                checkout.item_code,
                checkout.item_name,
                COALESCE(NULLIF(btrim(items.item_group), ''), '') AS item_group,
                checkout.qolip_code,
                checkout.size,
                COALESCE(checkout.payload_json->>'color', '') AS color,
                checkout.created_at
            FROM mini_qolip_checkouts checkout
            LEFT JOIN mini_items items
              ON lower(items.code) = lower(checkout.item_code)
            WHERE lower(checkout.status) = 'open'
              AND NOT EXISTS (
                  SELECT 1
                  FROM mini_qolip_product_specs spec
                  WHERE lower(spec.qolip_code) = lower(checkout.qolip_code)
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM mini_qolip_locations location
                  WHERE lower(location.qolip_code) = lower(checkout.qolip_code)
              )
            ORDER BY lower(checkout.qolip_code), checkout.updated_at DESC, checkout.issued_at DESC
        ),
        qolip_sources AS (
            SELECT
                spec.item_code,
                spec.item_name,
                spec.item_group,
                spec.qolip_code,
                spec.size,
                COALESCE(spec.payload_json->>'color', '') AS color,
                spec.created_at
            FROM mini_qolip_product_specs spec
            UNION ALL
            SELECT
                location.item_code,
                location.item_name,
                location.item_group,
                location.qolip_code,
                location.size,
                location.color,
                location.created_at
            FROM legacy_locations location
            UNION ALL
            SELECT
                checkout.item_code,
                checkout.item_name,
                checkout.item_group,
                checkout.qolip_code,
                checkout.size,
                checkout.color,
                checkout.created_at
            FROM legacy_checkouts checkout
        ),
        product_rows AS (
            SELECT
                COALESCE(items.code, source.item_code) AS code,
                COALESCE(
                    NULLIF(btrim(items.name), ''),
                    NULLIF(btrim(source.item_name), ''),
                    source.item_code
                ) AS name,
                COALESCE(
                    NULLIF(btrim(items.item_group), ''),
                    NULLIF(btrim(source.item_group), ''),
                    ''
                ) AS item_group,
                source.qolip_code,
                COALESCE(
                    NULLIF(btrim(items.payload_json->>'qolip_first_code'), ''),
                    FIRST_VALUE(source.qolip_code) OVER (
                        PARTITION BY lower(COALESCE(items.code, source.item_code))
                        ORDER BY source.created_at ASC NULLS LAST,
                                 lower(source.qolip_code)
                    ),
                    ''
                ) AS first_qolip_code,
                source.size,
                source.color,
                source.qolip_code IS NOT NULL AS has_qolip_spec
            FROM eligible_items items
            FULL OUTER JOIN qolip_sources source
              ON lower(source.item_code) = lower(items.code)
        )
        SELECT
            product.code,
            product.name,
            product.item_group,
            COALESCE((
                SELECT array_agg(
                    CASE WHEN btrim(customers.name) <> ''
                         THEN customers.name ELSE customers.ref END
                    ORDER BY lower(customers.name), customers.ref
                )
                FROM mini_customer_items assignments
                JOIN mini_customers customers
                  ON customers.ref = assignments.customer_ref
                WHERE lower(assignments.item_code) = lower(product.code)
            ), ARRAY[]::text[]) AS customer_names,
            COALESCE(product.qolip_code, '') AS qolip_code,
            product.first_qolip_code,
            COALESCE(product.size, 0) AS size,
            COALESCE(product.color, '') AS color,
            product.has_qolip_spec,
            EXISTS (
                SELECT 1
                FROM mini_qolip_checkouts checkout
                WHERE lower(checkout.qolip_code) = lower(product.qolip_code)
                  AND lower(checkout.status) = 'open'
            ) OR EXISTS (
                SELECT 1
                FROM mini_order_run_sessions session
                WHERE session.status IN ('active', 'paused', 'roll_detached')
                  AND (
                      lower(session.payload_json->>'qolip_code') = lower(product.qolip_code)
                      OR EXISTS (
                          SELECT 1
                          FROM jsonb_array_elements_text(
                              CASE
                                  WHEN jsonb_typeof(session.payload_json->'qolip_codes') = 'array'
                                  THEN session.payload_json->'qolip_codes'
                                  ELSE '[]'::jsonb
                              END
                          ) AS code(value)
                          WHERE lower(code.value) = lower(product.qolip_code)
                      )
                  )
            ) AS is_in_use
        FROM product_rows product
        WHERE (NOT $4 OR product.has_qolip_spec)
          AND (
            $1 = ''
            OR lower(product.code) LIKE $2
            OR lower(product.name) LIKE $2
            OR lower(COALESCE(product.qolip_code, '')) LIKE $2
            OR EXISTS (
                SELECT 1
                FROM mini_customer_items assignments
                JOIN mini_customers customers
                  ON customers.ref = assignments.customer_ref
                WHERE lower(assignments.item_code) = lower(product.code)
                  AND (
                    lower(customers.name) LIKE $2
                    OR lower(customers.ref) LIKE $2
                  )
            )
          )
        ORDER BY lower(product.name), lower(product.code), lower(COALESCE(product.qolip_code, ''))
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
            customer_names: row.customer_names,
            qolip_code: row.qolip_code,
            first_qolip_code: row.first_qolip_code,
            size: row.size,
            color: row.color,
            has_qolip_spec: row.has_qolip_spec,
            is_in_use: row.is_in_use,
        })
        .collect())
}

pub(super) async fn load_product_spec(
    pool: &PgPool,
    item_code: &str,
) -> Result<Option<QolipProductSpec>, QolipError> {
    Ok(load_product_specs(pool, item_code)
        .await?
        .into_iter()
        .next())
}

pub(super) async fn load_product_specs(
    pool: &PgPool,
    item_code: &str,
) -> Result<Vec<QolipProductSpec>, QolipError> {
    let rows = sqlx::query_as::<_, QolipProductSpecRow>(
        r#"
        SELECT item_code, item_name, item_group, qolip_code, size, color,
               created_by_role, created_by_ref, created_by_name
        FROM (
            SELECT
                spec.item_code,
                spec.item_name,
                spec.item_group,
                spec.qolip_code,
                spec.size,
                COALESCE(spec.payload_json->>'color', '') AS color,
                spec.created_by_role,
                spec.created_by_ref,
                spec.created_by_name,
                0 AS source_priority,
                spec.updated_at
            FROM mini_qolip_product_specs spec
            WHERE lower(spec.item_code) = lower($1)
            UNION ALL
            SELECT
                location.item_code,
                location.item_name,
                COALESCE(NULLIF(btrim(items.item_group), ''), '') AS item_group,
                location.qolip_code,
                location.size,
                COALESCE(location.payload_json->>'color', '') AS color,
                location.created_by_role,
                location.created_by_ref,
                location.created_by_name,
                1 AS source_priority,
                location.updated_at
            FROM mini_qolip_locations location
            LEFT JOIN mini_items items
              ON lower(items.code) = lower(location.item_code)
            WHERE lower(location.item_code) = lower($1)
              AND NOT EXISTS (
                  SELECT 1
                  FROM mini_qolip_product_specs spec
                  WHERE lower(spec.qolip_code) = lower(location.qolip_code)
              )
            UNION ALL
            SELECT
                checkout.item_code,
                checkout.item_name,
                COALESCE(NULLIF(btrim(items.item_group), ''), '') AS item_group,
                checkout.qolip_code,
                checkout.size,
                COALESCE(checkout.payload_json->>'color', '') AS color,
                checkout.issued_by_role AS created_by_role,
                checkout.issued_by_ref AS created_by_ref,
                checkout.issued_by_name AS created_by_name,
                2 AS source_priority,
                checkout.updated_at
            FROM mini_qolip_checkouts checkout
            LEFT JOIN mini_items items
              ON lower(items.code) = lower(checkout.item_code)
            WHERE lower(checkout.item_code) = lower($1)
              AND lower(checkout.status) = 'open'
              AND NOT EXISTS (
                  SELECT 1
                  FROM mini_qolip_product_specs spec
                  WHERE lower(spec.qolip_code) = lower(checkout.qolip_code)
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM mini_qolip_locations location
                  WHERE lower(location.qolip_code) = lower(checkout.qolip_code)
              )
        ) candidates
        ORDER BY lower(qolip_code), source_priority, updated_at DESC
        "#,
    )
    .bind(item_code.trim())
    .fetch_all(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(rows.into_iter().map(row_to_product_spec).collect())
}

pub(super) async fn load_product_spec_by_qolip_code(
    pool: &PgPool,
    qolip_code: &str,
) -> Result<Option<QolipProductSpec>, QolipError> {
    let row = sqlx::query_as::<_, QolipProductSpecRow>(
        r#"
        SELECT item_code, item_name, item_group, qolip_code, size, color,
               created_by_role, created_by_ref, created_by_name
        FROM (
            SELECT
                spec.item_code,
                spec.item_name,
                spec.item_group,
                spec.qolip_code,
                spec.size,
                COALESCE(spec.payload_json->>'color', '') AS color,
                spec.created_by_role,
                spec.created_by_ref,
                spec.created_by_name,
                0 AS source_priority,
                spec.updated_at
            FROM mini_qolip_product_specs spec
            WHERE lower(spec.qolip_code) = lower($1)
            UNION ALL
            SELECT
                location.item_code,
                location.item_name,
                COALESCE(NULLIF(btrim(items.item_group), ''), '') AS item_group,
                location.qolip_code,
                location.size,
                COALESCE(location.payload_json->>'color', '') AS color,
                location.created_by_role,
                location.created_by_ref,
                location.created_by_name,
                1 AS source_priority,
                location.updated_at
            FROM mini_qolip_locations location
            LEFT JOIN mini_items items
              ON lower(items.code) = lower(location.item_code)
            WHERE lower(location.qolip_code) = lower($1)
              AND NOT EXISTS (
                  SELECT 1
                  FROM mini_qolip_product_specs spec
                  WHERE lower(spec.qolip_code) = lower(location.qolip_code)
              )
            UNION ALL
            SELECT
                checkout.item_code,
                checkout.item_name,
                COALESCE(NULLIF(btrim(items.item_group), ''), '') AS item_group,
                checkout.qolip_code,
                checkout.size,
                COALESCE(checkout.payload_json->>'color', '') AS color,
                checkout.issued_by_role AS created_by_role,
                checkout.issued_by_ref AS created_by_ref,
                checkout.issued_by_name AS created_by_name,
                2 AS source_priority,
                checkout.updated_at
            FROM mini_qolip_checkouts checkout
            LEFT JOIN mini_items items
              ON lower(items.code) = lower(checkout.item_code)
            WHERE lower(checkout.qolip_code) = lower($1)
              AND lower(checkout.status) = 'open'
              AND NOT EXISTS (
                  SELECT 1
                  FROM mini_qolip_product_specs spec
                  WHERE lower(spec.qolip_code) = lower(checkout.qolip_code)
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM mini_qolip_locations location
                  WHERE lower(location.qolip_code) = lower(checkout.qolip_code)
              )
        ) candidates
        ORDER BY source_priority, updated_at DESC
        LIMIT 1
        "#,
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
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;
    let saved = save_product_spec_tx(&mut tx, spec).await?;
    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(saved)
}

pub(super) async fn save_product_specs(
    pool: &PgPool,
    specs: Vec<QolipProductSpec>,
) -> Result<Vec<QolipProductSpec>, QolipError> {
    if specs.is_empty() {
        return Err(QolipError::MissingQolipCode);
    }
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;
    let mut saved = Vec::with_capacity(specs.len());
    for spec in specs {
        saved.push(save_product_spec_tx(&mut tx, spec).await?);
    }
    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(saved)
}

async fn save_product_spec_tx(
    tx: &mut Transaction<'_, Postgres>,
    mut spec: QolipProductSpec,
) -> Result<QolipProductSpec, QolipError> {
    let normalized_qolip_code = spec.qolip_code.trim().to_lowercase();
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM mini_qolip_product_specs
             WHERE lower(qolip_code) = $1
         )",
    )
    .bind(&normalized_qolip_code)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    if exists {
        return Err(QolipError::QolipCodeConflict);
    }
    sqlx::query(
        "UPDATE mini_items item
         SET payload_json = jsonb_set(
                 COALESCE(item.payload_json, '{}'::jsonb),
                 '{qolip_first_code}',
                 to_jsonb(COALESCE(
                     (
                         SELECT existing.qolip_code
                         FROM mini_qolip_product_specs existing
                         WHERE lower(existing.item_code) = lower($1)
                         ORDER BY existing.created_at ASC, lower(existing.qolip_code)
                         LIMIT 1
                     ),
                     $2
                 )::text),
                 true
             ),
             updated_at = now()
         WHERE lower(item.code) = lower($1)
           AND COALESCE(btrim(item.payload_json->>'qolip_first_code'), '') = ''",
    )
    .bind(spec.item_code.trim())
    .bind(spec.qolip_code.trim())
    .execute(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    spec.color = allocate_panton_color(tx, &spec.qolip_code, None, &spec.color).await?;
    let row = sqlx::query_as::<_, QolipProductSpecRow>(
        "INSERT INTO mini_qolip_product_specs (
             item_code, item_name, item_group, qolip_code, size,
             created_by_role, created_by_ref, created_by_name, payload_json
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT (lower(qolip_code)) DO NOTHING
         RETURNING item_code, item_name, item_group, qolip_code, size,
             COALESCE(payload_json->>'color', '') AS color,
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
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?
    .ok_or(QolipError::QolipCodeConflict)?;
    Ok(row_to_product_spec(row))
}

async fn allocate_panton_color(
    tx: &mut Transaction<'_, Postgres>,
    qolip_code: &str,
    excluded_qolip_code: Option<&str>,
    requested_color: &str,
) -> Result<String, QolipError> {
    let requested = requested_color.trim();
    if !requested.to_ascii_uppercase().starts_with("PANTON") {
        return Ok(requested.to_string());
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('qolip-panton-global')::bigint)")
        .execute(&mut **tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    let colors = sqlx::query_scalar::<_, String>(
        "SELECT color
         FROM (
             SELECT qolip_code, COALESCE(payload_json->>'color', '') AS color
             FROM mini_qolip_product_specs
             UNION ALL
             SELECT qolip_code, COALESCE(payload_json->>'color', '') AS color
             FROM mini_qolip_locations
             UNION ALL
             SELECT qolip_code, COALESCE(payload_json->>'color', '') AS color
             FROM mini_qolip_checkouts
         ) colors
         WHERE ($1 = '' OR lower(qolip_code) <> lower($1))
           AND ($2 = '' OR lower(qolip_code) <> lower($2))",
    )
    .bind(qolip_code.trim())
    .bind(excluded_qolip_code.unwrap_or_default().trim())
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    let used = colors
        .iter()
        .filter_map(|color| panton_number(color))
        .collect::<std::collections::BTreeSet<_>>();
    let requested_number = panton_number(requested);
    let number = requested_number
        .filter(|number| !used.contains(number))
        .or_else(|| (1..=QOLIP_PANTON_MAX_NUMBER).find(|number| !used.contains(number)))
        .ok_or(QolipError::PantonLimitExceeded)?;
    Ok(format!("Panton {number}"))
}

fn panton_number(color: &str) -> Option<i32> {
    let mut parts = color.split_whitespace();
    if !parts.next()?.eq_ignore_ascii_case("panton") {
        return None;
    }
    match parts.next()?.parse::<i32>().ok()? {
        number @ 1..=QOLIP_PANTON_MAX_NUMBER if parts.next().is_none() => Some(number),
        _ => None,
    }
}

pub(super) async fn rename_product_spec(
    pool: &PgPool,
    previous_qolip_code: &str,
    mut spec: QolipProductSpec,
) -> Result<QolipProductSpec, QolipError> {
    let previous = previous_qolip_code.trim().to_lowercase();
    let next = spec.qolip_code.trim().to_lowercase();
    if previous.is_empty() || next.is_empty() {
        return Err(QolipError::MissingQolipCode);
    }
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;
    spec.color =
        allocate_panton_color(&mut tx, &spec.qolip_code, Some(&previous), &spec.color).await?;
    for code in [&previous, &next] {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1)::bigint)")
            .bind(code)
            .execute(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
    }
    if previous != next {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                 SELECT 1 FROM mini_qolip_product_specs WHERE lower(qolip_code) = $1
                 UNION ALL
                 SELECT 1 FROM mini_qolip_locations WHERE lower(qolip_code) = $1
                 UNION ALL
                 SELECT 1 FROM mini_qolip_checkouts
                 WHERE lower(qolip_code) = $1 AND lower(status) = 'open'
                 UNION ALL
                 SELECT 1 FROM mini_order_run_sessions
                 WHERE status IN ('active', 'paused', 'roll_detached')
                   AND (
                       lower(payload_json->>'qolip_code') = $1
                       OR EXISTS (
                           SELECT 1
                           FROM jsonb_array_elements_text(
                               CASE
                                   WHEN jsonb_typeof(payload_json->'qolip_codes') = 'array'
                                   THEN payload_json->'qolip_codes'
                                   ELSE '[]'::jsonb
                               END
                           ) AS code(value)
                           WHERE lower(code.value) = $1
                       )
                   )
             )",
        )
        .bind(&next)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
        if exists {
            return Err(QolipError::QolipCodeConflict);
        }
    }
    let blocked = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM mini_qolip_checkouts
             WHERE lower(qolip_code) = $1 AND lower(status) = 'open'
             UNION ALL
             SELECT 1 FROM mini_order_run_sessions
             WHERE status IN ('active', 'paused', 'roll_detached')
               AND (
                   lower(payload_json->>'qolip_code') = $1
                   OR EXISTS (
                       SELECT 1
                       FROM jsonb_array_elements_text(
                           CASE
                               WHEN jsonb_typeof(payload_json->'qolip_codes') = 'array'
                               THEN payload_json->'qolip_codes'
                               ELSE '[]'::jsonb
                           END
                       ) AS code(value)
                       WHERE lower(code.value) = $1
                   )
               )
         )",
    )
    .bind(&previous)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    if blocked {
        return Err(QolipError::QolipInUse);
    }
    let locations = sqlx::query_as::<_, (String, String, String, String, String, Option<i32>)>(
        "SELECT id, block, item_code, item_name, row_letter, column_number
         FROM mini_qolip_locations
         WHERE lower(qolip_code) = $1
         FOR UPDATE",
    )
    .bind(&previous)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    let location_updates = locations
        .into_iter()
        .map(
            |(old_id, block, item_code, item_name, row_letter, column_number)| {
                let new_id = qolip_location_id(
                    &block,
                    &item_code,
                    &spec.qolip_code,
                    spec.size,
                    &row_letter,
                    column_number,
                );
                (
                    old_id,
                    new_id,
                    item_code,
                    item_name,
                    row_letter,
                    column_number,
                )
            },
        )
        .collect::<Vec<_>>();
    for (old_id, new_id, item_code, item_name, _row_letter, _column_number) in &location_updates {
        if old_id != new_id {
            let id_conflict = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM mini_qolip_locations WHERE id = $1 AND id <> $2)",
            )
            .bind(new_id)
            .bind(old_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
            if id_conflict {
                return Err(QolipError::QolipCodeConflict);
            }
        }
        sqlx::query(
            "UPDATE mini_qolip_locations
             SET id = $2,
                 item_code = $3,
                 item_name = $4,
                 qolip_code = $5,
                 size = $6,
                 payload_json = jsonb_set(
                     jsonb_set(
                         jsonb_set(
                             jsonb_set(
                                 jsonb_set(COALESCE(payload_json, '{}'::jsonb), '{id}', to_jsonb($2::text), true),
                                 '{item_code}', to_jsonb($3::text), true
                             ),
                             '{item_name}', to_jsonb($4::text), true
                         ),
                         '{qolip_code}', to_jsonb($5::text), true
                     ),
                     '{size}', to_jsonb($6::integer), true
                 ),
                 updated_at = now()
             WHERE id = $1",
        )
        .bind(old_id)
        .bind(new_id)
        .bind(item_code)
        .bind(item_name)
        .bind(spec.qolip_code.trim())
        .bind(spec.size)
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    }
    let row = sqlx::query_as::<_, QolipProductSpecRow>(
        "UPDATE mini_qolip_product_specs
         SET item_code = $2, item_name = $3, item_group = $4, qolip_code = $5,
             size = $6, created_by_role = $7, created_by_ref = $8,
             created_by_name = $9, payload_json = $10, updated_at = now()
         WHERE lower(qolip_code) = $1
         RETURNING item_code, item_name, item_group, qolip_code, size,
             COALESCE(payload_json->>'color', '') AS color,
             created_by_role, created_by_ref, created_by_name",
    )
    .bind(&previous)
    .bind(spec.item_code.trim())
    .bind(spec.item_name.trim())
    .bind(spec.item_group.trim())
    .bind(spec.qolip_code.trim())
    .bind(spec.size)
    .bind(spec.created_by_role.trim())
    .bind(spec.created_by_ref.trim())
    .bind(spec.created_by_name.trim())
    .bind(serde_json::to_value(&spec).map_err(|_| QolipError::StoreFailed)?)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?
    .ok_or(QolipError::QolipCodeNotFound)?;
    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
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

    let _locked_specs = sqlx::query_scalar::<_, String>(
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
    let _locked_locations = sqlx::query_scalar::<_, String>(
        "SELECT qolip_code
         FROM mini_qolip_locations
         WHERE lower(qolip_code) = ANY($1)
         ORDER BY lower(qolip_code), id
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
             UNION ALL
             SELECT 1
             FROM mini_order_run_sessions
             WHERE status IN ('active', 'paused', 'roll_detached')
               AND (
                   lower(payload_json->>'qolip_code') = ANY($1)
                   OR EXISTS (
                       SELECT 1
                       FROM jsonb_array_elements_text(
                           CASE
                               WHEN jsonb_typeof(payload_json->'qolip_codes') = 'array'
                               THEN payload_json->'qolip_codes'
                               ELSE '[]'::jsonb
                           END
                       ) AS code(value)
                       WHERE lower(code.value) = ANY($1)
                   )
               )
         )",
    )
    .bind(&normalized)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    if in_use {
        return Err(QolipError::QolipInUse);
    }

    let deleted = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint
         FROM (
             SELECT lower(qolip_code) AS code
             FROM mini_qolip_product_specs
             WHERE lower(qolip_code) = ANY($1)
             UNION
             SELECT lower(qolip_code) AS code
             FROM mini_qolip_locations
             WHERE lower(qolip_code) = ANY($1)
         ) existing",
    )
    .bind(&normalized)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)? as usize;

    sqlx::query("DELETE FROM mini_qolip_locations WHERE lower(qolip_code) = ANY($1)")
        .bind(&normalized)
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    sqlx::query("DELETE FROM mini_qolip_product_specs WHERE lower(qolip_code) = ANY($1)")
        .bind(&normalized)
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(deleted)
}
