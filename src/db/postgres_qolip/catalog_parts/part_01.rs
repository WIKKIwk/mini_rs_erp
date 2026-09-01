
const QOLIP_PANTON_MAX_NUMBER: i32 = 100;

pub(super) async fn load_assigned_warehouses(
    pool: &PgPool,
    principal: &Principal,
) -> Result<Vec<String>, QolipError> {
    let rows = sqlx::query_scalar::<_, String>(
        r#"
        SELECT warehouse_name
        FROM mini_warehouse_assignments
        WHERE principal_ref = $1
          AND lower(principal_role) = lower($2)
          AND assignment_kind = 'warehouse'
          AND btrim(warehouse_name) <> ''
        ORDER BY lower(warehouse_name)
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
            SELECT warehouse_name
            FROM mini_warehouse_assignments
            WHERE principal_ref = $1
              AND lower(principal_role) = lower($2)
              AND assignment_kind = 'warehouse'
        ),
        child_blocks AS (
            SELECT child.name AS block, assigned.warehouse_name AS warehouse
            FROM assigned
            JOIN mini_warehouses child
              ON lower(child.parent_warehouse) = lower(assigned.warehouse_name)
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
    let block_key = block.to_lowercase();
    let new_block_key = new_block.to_lowercase();
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtext(lock_key)::bigint)
         FROM (
             SELECT DISTINCT lock_key
             FROM unnest(ARRAY[$1::text, $2::text]) AS keys(lock_key)
             ORDER BY lock_key
         ) AS ordered_lock_keys",
    )
    .bind(&block_key)
    .bind(&new_block_key)
    .execute(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

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
         SET warehouse_name = $2,
             warehouse = $2,
             payload_json = jsonb_set(
                 COALESCE(payload_json, '{}'::jsonb),
                 '{warehouse}',
                 to_jsonb($2::text),
                 true
             ),
             updated_at = now()
         WHERE assignment_kind = 'warehouse'
           AND lower(warehouse_name) = lower($1)",
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
