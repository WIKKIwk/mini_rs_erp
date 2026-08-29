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
                WHERE session.status IN ('active', 'paused', 'frozen', 'roll_detached')
                  AND session.payload_json->>'qolip_lock_owner' = 'true'
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
