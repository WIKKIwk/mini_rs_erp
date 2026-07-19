use sqlx::{Postgres, Transaction};

use crate::core::production_map::{OrderRunSession, OrderRunStatus, ProductionMapError};

pub(super) async fn reject_qolip_in_use_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<(), ProductionMapError> {
    if !matches!(
        session.status,
        OrderRunStatus::Active | OrderRunStatus::Paused
    ) {
        return Ok(());
    }
    let Some(qolip_code) = session
        .payload_json
        .get("qolip_code")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let lock_key = format!("qolip:{}", qolip_code.to_ascii_lowercase());
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock_key)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let already_in_use = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1
            FROM mini_order_run_sessions
            WHERE status IN ('active', 'paused')
              AND lower(payload_json->>'qolip_code') = lower($1)
              AND session_id <> $2
         )",
    )
    .bind(qolip_code)
    .bind(session.session_id.trim())
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if already_in_use {
        return Err(ProductionMapError::QolipAlreadyInUse);
    }
    Ok(())
}
