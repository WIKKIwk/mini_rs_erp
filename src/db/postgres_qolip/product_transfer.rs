use sqlx::{PgPool, Postgres, Transaction};

use super::rows::{QolipProductSpecRow, row_to_product_spec};
use crate::core::auth::models::Principal;
use crate::core::qolip::{QolipError, QolipProductSpec, QolipProductTransfer};

pub(super) async fn transfer(
    pool: &PgPool,
    input: &QolipProductTransfer,
    principal: &Principal,
    allowed: &[String],
) -> Result<Vec<QolipProductSpec>, QolipError> {
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;
    lock_composition(&mut tx).await?;
    let actor = format!("qolipchi:{}", principal.ref_.trim());
    let event_id = format!("qolip-product-transfer:{actor}:{}", input.request_id);
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&event_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    if let Some(payload) = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json FROM mini_engine_events WHERE event_id = $1",
    )
    .bind(&event_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?
    {
        let previous: QolipProductTransfer = serde_json::from_value(payload["request"].clone())
            .map_err(|_| QolipError::StoreFailed)?;
        if previous != *input {
            return Err(QolipError::ProductTransferConflict);
        }
        let specs: Vec<QolipProductSpec> = serde_json::from_value(payload["specs"].clone())
            .map_err(|_| QolipError::StoreFailed)?;
        for spec in &specs {
            ensure_owner(spec, allowed)?;
        }
        return Ok(specs);
    }
    // Match both production-session locks and warehouse checkout/edit locks.
    // Take the whole batch in a stable order before checking or changing it.
    for code in &input.qolip_codes {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("qolip:{}", code.to_ascii_lowercase()))
            .execute(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
    }
    for code in &input.qolip_codes {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext(lower($1))::bigint)")
            .bind(code)
            .execute(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
    }
    let products = sqlx::query_as::<_, (String, String, String)>(
        "SELECT code, name, item_group FROM mini_items
         WHERE lower(btrim(code)) = ANY($1) ORDER BY code FOR UPDATE",
    ).bind(vec![input.from_item_code.to_lowercase(), input.to_item_code.to_lowercase()])
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    let source = products.iter().find(|product| product.0.eq_ignore_ascii_case(&input.from_item_code))
        .ok_or(QolipError::MissingItem)?;
    let target = products.iter().find(|product| product.0.eq_ignore_ascii_case(&input.to_item_code))
    .ok_or(QolipError::MissingItem)?;
    if target.2.trim().is_empty() {
        return Err(QolipError::MissingItemGroup);
    }

    reject_product_in_use(&mut tx, &source.0).await?;
    reject_product_in_use(&mut tx, &target.0).await?;
    let mut previous = Vec::new();
    let mut specs = Vec::new();
    for code in &input.qolip_codes {
        reject_in_use(&mut tx, code).await?;
        let old = load_spec(&mut tx, code).await?;
        ensure_owner(&old, allowed)?;
        if old.item_code.eq_ignore_ascii_case(&target.0)
            && !old.item_group.eq_ignore_ascii_case(&target.2) {
            return Err(QolipError::ProductTransferConflict);
        }
        let linked = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM mini_qolip_effective_product_molds
             WHERE qolip_code_key = $1 AND item_code_key = lower(btrim($2)))"
        ).bind(code).bind(&source.0).fetch_one(&mut *tx).await
            .map_err(|_| QolipError::StoreFailed)?;
        if !linked { return Err(QolipError::ProductTransferConflict); }
        let already_linked = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM mini_qolip_effective_product_molds
             WHERE qolip_code_key = $1 AND item_code_key = lower(btrim($2)))"
        ).bind(code).bind(&target.0).fetch_one(&mut *tx).await
            .map_err(|_| QolipError::StoreFailed)?;
        if already_linked { return Err(QolipError::ProductTransferConflict); }
        let target_base = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM mini_qolip_set_products
             WHERE set_id = $1 AND item_code_key = lower(btrim($2)))"
        ).bind(old.set_id()).bind(&target.0).fetch_one(&mut *tx).await
            .map_err(|_| QolipError::StoreFailed)?;
        let mut projected = old.clone();
        projected.is_extension = !target_base;
        projected.item_code = target.0.clone();
        projected.item_name = target.1.clone();
        projected.item_group = target.2.clone();
        // Keep a product-local exclusion even for a removed extension. A later
        // physical catalog edit must not silently reintroduce this mold.
        write_override(&mut tx, &source.0, &old.qolip_code, "exclude", principal).await?;
        if target_base {
            sqlx::query("DELETE FROM mini_qolip_product_overrides
                WHERE item_code_key = lower(btrim($1)) AND qolip_code_key = $2")
                .bind(&target.0).bind(code).execute(&mut *tx).await.map_err(|_| QolipError::StoreFailed)?;
        } else {
            write_override(&mut tx, &target.0, &old.qolip_code, "include", principal).await?;
        }
        previous.push(old);
        specs.push(projected);
    }
    // Record the correction and its response in the same transaction. A lost
    // response can be retried without moving the QR twice or rewriting history.
    sqlx::query(
        "INSERT INTO mini_engine_events
            (event_id, domain, action, entity_id, actor_key, idempotency_key, payload_json)
         VALUES ($1, 'qolip', 'product_transfer', $2, $3, $4, $5)",
    )
    .bind(&event_id)
    .bind(&target.0)
    .bind(&actor)
    .bind(&input.request_id)
    .bind(serde_json::json!({
        "request": input, "previous_specs": previous, "specs": specs,
        "actor_name": principal.display_name,
    }))
    .execute(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(specs)
}

pub(super) async fn lock_composition(tx: &mut Transaction<'_, Postgres>) -> Result<(), QolipError> {
    // Taken before physical mold locks, matching the session writer's shared lock.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('qolip:product-composition', 0))")
        .execute(&mut **tx).await.map_err(|_| QolipError::StoreFailed)?;
    Ok(())
}

pub(super) async fn reject_product_in_use(tx: &mut Transaction<'_, Postgres>, item_code: &str) -> Result<(), QolipError> {
    let busy = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM mini_order_run_sessions session
         JOIN mini_production_maps map ON map.id = session.order_id
         WHERE lower(btrim(map.product_code)) = lower(btrim($1))
           AND session.status IN ('active','paused','frozen','roll_detached')
           AND session.payload_json->>'qolip_lock_owner' = 'true')"
    ).bind(item_code).fetch_one(&mut **tx).await.map_err(|_| QolipError::StoreFailed)?;
    if busy { return Err(QolipError::QolipInUse); }
    Ok(())
}

pub(super) async fn write_override(tx: &mut Transaction<'_, Postgres>, item: &str, code: &str, mode: &str, principal: &Principal) -> Result<(), QolipError> {
    sqlx::query("INSERT INTO mini_qolip_product_overrides
        (item_code, qolip_code, mode, created_by_role, created_by_ref, created_by_name)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (item_code_key, qolip_code_key) DO UPDATE
        SET mode = EXCLUDED.mode, created_by_role = EXCLUDED.created_by_role,
            created_by_ref = EXCLUDED.created_by_ref, created_by_name = EXCLUDED.created_by_name, updated_at = now()")
        .bind(item).bind(code).bind(mode).bind(crate::core::qolip::role_code(&principal.role))
        .bind(&principal.ref_).bind(&principal.display_name).execute(&mut **tx).await.map_err(|_| QolipError::StoreFailed)?;
    Ok(())
}

fn ensure_owner(spec: &QolipProductSpec, allowed: &[String]) -> Result<(), QolipError> {
    if spec.warehouse.trim().is_empty()
        || !allowed
            .iter()
            .any(|warehouse| warehouse.trim().eq_ignore_ascii_case(spec.warehouse.trim()))
    {
        return Err(QolipError::AccessDenied);
    }
    Ok(())
}

pub(super) async fn load_spec(
    tx: &mut Transaction<'_, Postgres>,
    code: &str,
) -> Result<QolipProductSpec, QolipError> {
    let row = sqlx::query_as::<_, QolipProductSpecRow>(
        "SELECT item_code, item_name, item_group, qolip_code, size,
            COALESCE(payload_json->>'warehouse', '') AS warehouse,
            COALESCE(payload_json->>'color', '') AS color,
            COALESCE(payload_json->>'qolip_set_id', '') AS qolip_set_id,
            created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_product_specs WHERE lower(qolip_code) = $1 FOR UPDATE",
    )
    .bind(code)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    if let Some(row) = row {
        return Ok(row_to_product_spec(row));
    }
    // Keep the existing legacy-location lookup usable; preserve its owner and
    // creator when promoting the mold into a catalog specification.
    let row = sqlx::query_as::<_, QolipProductSpecRow>(
        "SELECT l.item_code, l.item_name, COALESCE(i.item_group, '') AS item_group,
            l.qolip_code, l.size,
            COALESCE(mini_qolip_assigned_warehouse(l.created_by_role, l.created_by_ref), '') AS warehouse,
            COALESCE(l.payload_json->>'color', '') AS color,
            l.created_by_role, l.created_by_ref, l.created_by_name
         FROM mini_qolip_locations l LEFT JOIN mini_items i ON lower(i.code) = lower(l.item_code)
         WHERE lower(l.qolip_code) = $1 ORDER BY l.updated_at DESC LIMIT 1 FOR UPDATE OF l",
    ).bind(code).fetch_optional(&mut **tx).await.map_err(|_| QolipError::StoreFailed)?
        .ok_or(QolipError::QolipCodeNotFound)?;
    Ok(row_to_product_spec(row))
}

pub(super) async fn reject_in_use(tx: &mut Transaction<'_, Postgres>, code: &str) -> Result<(), QolipError> {
    let blocked = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM mini_qolip_checkouts WHERE lower(qolip_code) = $1 AND status = 'open'
            UNION ALL
            SELECT 1 FROM mini_order_run_sessions s
            WHERE s.status IN ('active','paused','frozen','roll_detached')
              AND s.payload_json->>'qolip_lock_owner' = 'true'
              AND (lower(s.payload_json->>'qolip_code') = $1 OR EXISTS (
                  SELECT 1 FROM jsonb_array_elements_text(CASE
                      WHEN jsonb_typeof(s.payload_json->'qolip_codes') = 'array'
                      THEN s.payload_json->'qolip_codes' ELSE '[]'::jsonb END) AS c(value)
                  WHERE lower(c.value) = $1
              ))
         )",
    )
    .bind(code)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    if blocked {
        return Err(QolipError::QolipInUse);
    }
    Ok(())
}
