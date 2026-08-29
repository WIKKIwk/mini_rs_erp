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
