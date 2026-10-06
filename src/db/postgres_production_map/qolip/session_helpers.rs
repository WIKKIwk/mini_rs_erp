use sqlx::{Postgres, Transaction};

use crate::core::production_map::{OrderRunSession, OrderRunStatus, ProductionMapError};

pub(super) async fn reject_qolip_in_use_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<(), ProductionMapError> {
    if !matches!(
        session.status,
        OrderRunStatus::Active
            | OrderRunStatus::Paused
            | OrderRunStatus::Frozen
            | OrderRunStatus::RollDetached
    ) {
        return Ok(());
    }
    let mut qolip_codes = session
        .payload_json
        .get("qolip_codes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if let Some(qolip_code) = session
        .payload_json
        .get("qolip_code")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && !qolip_codes
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(qolip_code))
    {
        qolip_codes.push(qolip_code.to_string());
    }
    qolip_codes.sort_by_key(|code| code.to_ascii_lowercase());
    qolip_codes.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    if qolip_codes.is_empty() {
        return Ok(());
    }
    for qolip_code in qolip_codes {
        let lock_key = format!("qolip:{}", qolip_code.to_ascii_lowercase());
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut **tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let already_in_use = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1
                FROM mini_order_run_sessions AS session
                WHERE session.status IN ('active', 'paused', 'frozen', 'roll_detached')
                  AND session.session_id <> $2
                  AND session.payload_json->>'qolip_lock_owner' = 'true'
                  AND (
                    lower(session.payload_json->>'qolip_code') = lower($1)
                    OR EXISTS (
                        SELECT 1
                        FROM jsonb_array_elements_text(
                            CASE
                                WHEN jsonb_typeof(session.payload_json->'qolip_codes') = 'array'
                                THEN session.payload_json->'qolip_codes'
                                ELSE '[]'::jsonb
                            END
                        ) AS code(value)
                        WHERE lower(code.value) = lower($1)
                    )
                  )
             )",
        )
        .bind(&qolip_code)
        .bind(session.session_id.trim())
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if already_in_use {
            return Err(ProductionMapError::QolipAlreadyInUse);
        }
    }
    Ok(())
}

pub(super) async fn validate_qolip_set_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<(), ProductionMapError> {
    let Some(set_id) = session.payload_json.get("qolip_set_id")
        .and_then(serde_json::Value::as_str).filter(|id| !id.is_empty()) else {
        return Ok(());
    };
    let item_code = sqlx::query_scalar::<_, String>(
        "SELECT product_code FROM mini_production_maps WHERE id = $1",
    ).bind(&session.order_id).fetch_one(&mut **tx).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT qolip_code, COALESCE(payload_json->>'qolip_set_id', ''),
                COALESCE(payload_json->>'warehouse', '')
         FROM mini_qolip_product_specs WHERE lower(item_code) = lower($1)
           AND (payload_json->>'qolip_set_id' = $2 OR
                (COALESCE(btrim(payload_json->>'qolip_set_id'), '') = '' AND
                 'legacy:' || length(lower(btrim(item_code)))::text || ':' || lower(btrim(item_code))
                   || ':' || lower(btrim(COALESCE(payload_json->>'warehouse', ''))) = $2))
         ORDER BY lower(qolip_code) FOR SHARE",
    ).bind(&item_code).bind(set_id).fetch_all(&mut **tx).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let legacy = sqlx::query_as::<_, (String, String, String)>(
        "SELECT l.qolip_code, ''::text,
                COALESCE(mini_qolip_assigned_warehouse(l.created_by_role, l.created_by_ref), '')
         FROM mini_qolip_locations l WHERE lower(l.item_code) = lower($1)
           AND NOT EXISTS (SELECT 1 FROM mini_qolip_product_specs s WHERE lower(s.qolip_code) = lower(l.qolip_code))
         UNION ALL
         SELECT c.qolip_code, ''::text,
                COALESCE(mini_qolip_assigned_warehouse(c.issued_by_role, c.issued_by_ref), '')
         FROM mini_qolip_checkouts c WHERE lower(c.item_code) = lower($1) AND c.status = 'open'
           AND NOT EXISTS (SELECT 1 FROM mini_qolip_product_specs s WHERE lower(s.qolip_code) = lower(c.qolip_code))",
    ).bind(&item_code).fetch_all(&mut **tx).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.extend(legacy);
    let required = rows.into_iter().filter_map(|(code, id, warehouse)| {
        let spec = crate::core::qolip::QolipProductSpec {
            qolip_set_id: id, item_code: item_code.clone(), warehouse,
            ..Default::default()
        };
        (spec.set_id() == set_id).then(|| code.trim().to_lowercase())
    }).collect::<std::collections::BTreeSet<_>>();
    let scanned = session.payload_json.get("qolip_codes")
        .and_then(serde_json::Value::as_array).into_iter().flatten()
        .filter_map(serde_json::Value::as_str)
        .map(|code| code.trim().to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    if required.is_empty() || required != scanned {
        return Err(ProductionMapError::QolipCodeMismatch);
    }
    Ok(())
}
