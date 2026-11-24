async fn load_product_spec_by_qolip_code_inner(
    pool: &PgPool,
    qolip_code: &str,
    active_product: bool,
) -> Result<Option<QolipProductSpec>, QolipError> {
    let row = sqlx::query_as::<_, QolipProductSpecRow>(
        r#"
        SELECT COALESCE(active.item_code, candidates.item_code) AS item_code,
               COALESCE(active.item_name, candidates.item_name) AS item_name,
               COALESCE(active.item_group, candidates.item_group) AS item_group,
               candidates.qolip_code, candidates.size, candidates.warehouse, candidates.color,
               COALESCE((SELECT s.payload_json->>'qolip_set_id' FROM mini_qolip_product_specs s
                         WHERE lower(s.qolip_code) = lower(candidates.qolip_code)), '') AS qolip_set_id,
               candidates.created_by_role, candidates.created_by_ref, candidates.created_by_name
        FROM (
            SELECT
                spec.item_code,
                spec.item_name,
                spec.item_group,
                spec.qolip_code,
                spec.size,
                COALESCE(spec.payload_json->>'warehouse', '') AS warehouse,
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
                COALESCE(mini_qolip_assigned_warehouse(location.created_by_role, location.created_by_ref), '') AS warehouse,
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
                COALESCE(mini_qolip_assigned_warehouse(checkout.issued_by_role, checkout.issued_by_ref), '') AS warehouse,
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
        LEFT JOIN LATERAL (
            SELECT link.item_code,
                   COALESCE(NULLIF(btrim(item.name), ''),
                       CASE WHEN link.item_code_key = lower(btrim(candidates.item_code))
                            THEN candidates.item_name ELSE link.item_code END) AS item_name,
                   COALESCE(NULLIF(btrim(item.item_group), ''), candidates.item_group) AS item_group
            FROM mini_qolip_effective_product_molds link
            LEFT JOIN mini_items item ON item.code = link.item_code
            WHERE $2::boolean
              AND link.qolip_code_key = lower(btrim(candidates.qolip_code))
            -- Keep the original product only while its link is still active.
            -- Otherwise choose a stable active link, never an excluded product.
            ORDER BY (link.item_code_key = lower(btrim(candidates.item_code))) DESC,
                     link.is_extension, link.item_code_key, link.item_code
            LIMIT 1
        ) active ON true
        WHERE NOT $2::boolean OR candidates.source_priority <> 0
           OR active.item_code IS NOT NULL OR btrim(candidates.item_code) = ''
        ORDER BY candidates.source_priority, candidates.updated_at DESC
        LIMIT 1
        "#,
    )
    .bind(qolip_code.trim())
    .bind(active_product)
    .fetch_optional(pool)
    .await
    .map_err(super::reworks::storage_error)?;

    Ok(row.map(row_to_product_spec))
}

pub(super) async fn save_product_spec(
    pool: &PgPool,
    spec: QolipProductSpec,
) -> Result<QolipProductSpec, QolipError> {
    let mut tx = pool.begin().await.map_err(super::reworks::storage_error)?;
    super::product_transfer::lock_composition(&mut tx).await?;
    let saved = save_product_spec_tx(&mut tx, spec).await?;
    tx.commit().await.map_err(super::reworks::storage_error)?;
    Ok(saved)
}

pub(super) async fn save_product_specs(
    pool: &PgPool,
    specs: Vec<QolipProductSpec>,
) -> Result<Vec<QolipProductSpec>, QolipError> {
    if specs.is_empty() {
        return Err(QolipError::MissingQolipCode);
    }
    let mut tx = pool.begin().await.map_err(super::reworks::storage_error)?;
    super::product_transfer::lock_composition(&mut tx).await?;
    let mut saved = Vec::with_capacity(specs.len());
    for spec in specs {
        saved.push(save_product_spec_tx(&mut tx, spec).await?);
    }
    tx.commit().await.map_err(super::reworks::storage_error)?;
    Ok(saved)
}
