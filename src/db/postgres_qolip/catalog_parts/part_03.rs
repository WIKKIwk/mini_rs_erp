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
                 WHERE status IN ('active', 'paused', 'frozen', 'roll_detached')
                   AND payload_json->>'qolip_lock_owner' = 'true'
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
             WHERE status IN ('active', 'paused', 'frozen', 'roll_detached')
               AND payload_json->>'qolip_lock_owner' = 'true'
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
             WHERE status IN ('active', 'paused', 'frozen', 'roll_detached')
               AND payload_json->>'qolip_lock_owner' = 'true'
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
