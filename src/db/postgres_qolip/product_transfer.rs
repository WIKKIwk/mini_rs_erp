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
    let target = sqlx::query_as::<_, (String, String, String)>(
        "SELECT code, name, item_group FROM mini_items WHERE lower(code) = lower($1) FOR UPDATE",
    )
    .bind(&input.to_item_code)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?
    .ok_or(QolipError::MissingItem)?;
    if target.2.trim().is_empty() {
        return Err(QolipError::MissingItemGroup);
    }

    let mut previous = Vec::new();
    let mut transferred_sets = std::collections::BTreeMap::new();
    let mut specs = Vec::new();
    for code in &input.qolip_codes {
        reject_in_use(&mut tx, code).await?;
        let old = load_spec(&mut tx, code).await?;
        ensure_owner(&old, allowed)?;
        if !old
            .item_code
            .trim()
            .eq_ignore_ascii_case(&input.from_item_code)
        {
            return Err(QolipError::ProductTransferConflict);
        }
        let mut spec = old.clone();
        spec.qolip_set_id = transferred_sets.entry(old.set_id())
            .or_insert_with(|| format!("qolip-set:{:032x}", rand::random::<u128>())).clone();
        spec.item_code = target.0.clone();
        spec.item_name = target.1.clone();
        spec.item_group = target.2.clone();
        previous.push(old);
        specs.push(spec);
    }
    for spec in &specs {
        super::catalog_locations::update_product_locations_tx(
            &mut tx,
            &spec.qolip_code.to_lowercase(),
            spec,
        )
        .await?;
        sqlx::query(
            "INSERT INTO mini_qolip_product_specs
                (item_code, item_name, item_group, qolip_code, size,
                 created_by_role, created_by_ref, created_by_name, payload_json)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
             ON CONFLICT (lower(qolip_code)) DO UPDATE SET
                item_code = EXCLUDED.item_code, item_name = EXCLUDED.item_name,
                item_group = EXCLUDED.item_group, payload_json = EXCLUDED.payload_json,
                updated_at = now()",
        )
        .bind(&spec.item_code)
        .bind(&spec.item_name)
        .bind(&spec.item_group)
        .bind(&spec.qolip_code)
        .bind(spec.size)
        .bind(&spec.created_by_role)
        .bind(&spec.created_by_ref)
        .bind(&spec.created_by_name)
        .bind(serde_json::to_value(spec).map_err(|_| QolipError::StoreFailed)?)
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    }
    sqlx::query(
        "UPDATE mini_items SET payload_json = jsonb_set(
             COALESCE(payload_json, '{}'::jsonb), '{qolip_first_code}', to_jsonb($2::text), true
         ), updated_at = now()
         WHERE code = $1 AND COALESCE(btrim(payload_json->>'qolip_first_code'), '') = ''",
    )
    .bind(&target.0)
    .bind(&specs[0].qolip_code)
    .execute(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
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

async fn load_spec(
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

async fn reject_in_use(tx: &mut Transaction<'_, Postgres>, code: &str) -> Result<(), QolipError> {
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
