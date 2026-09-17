use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    PrintPreflightHold, PrintPreflightStatus, ProductionMapError, QueueActionActor,
};

use super::transaction_locks::lock_order_and_apparatuses_tx;

#[derive(sqlx::FromRow)]
struct PrintPreflightHoldRow {
    hold_id: String,
    idempotency_key: String,
    order_id: String,
    canonical_apparatus_id: String,
    stage_node_id: String,
    status: String,
    actor_role: String,
    actor_ref: String,
    actor_display_name: String,
    created_at_unix: i64,
    updated_at_unix: i64,
    previous_queue_state: Option<String>,
    expires_at_unix: i64,
}

const HOLD_COLUMNS: &str = "hold_id, idempotency_key, order_id, canonical_apparatus_id,
    stage_node_id, status, actor_role, actor_ref, actor_display_name,
    created_at_unix, updated_at_unix, expires_at_unix, previous_queue_state";

pub(super) async fn load_active(
    pool: &PgPool,
) -> Result<Vec<PrintPreflightHold>, ProductionMapError> {
    let query = format!(
        "SELECT {HOLD_COLUMNS} FROM mini_print_preflight_holds
         WHERE status IN ('held', 'running', 'passed')
         ORDER BY created_at_unix, hold_id"
    );
    let rows = sqlx::query_as::<_, PrintPreflightHoldRow>(&query)
        .fetch_all(pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(row_to_hold).collect()
}

pub(super) async fn load_by_id(
    pool: &PgPool,
    hold_id: &str,
) -> Result<Option<PrintPreflightHold>, ProductionMapError> {
    let query = format!("SELECT {HOLD_COLUMNS} FROM mini_print_preflight_holds WHERE hold_id = $1");
    let row = sqlx::query_as::<_, PrintPreflightHoldRow>(&query)
        .bind(hold_id.trim())
        .fetch_optional(pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    row.map(row_to_hold).transpose()
}

pub(super) async fn load_by_idempotency_key(
    pool: &PgPool,
    idempotency_key: &str,
) -> Result<Option<PrintPreflightHold>, ProductionMapError> {
    let query =
        format!("SELECT {HOLD_COLUMNS} FROM mini_print_preflight_holds WHERE idempotency_key = $1");
    let row = sqlx::query_as::<_, PrintPreflightHoldRow>(&query)
        .bind(idempotency_key.trim())
        .fetch_optional(pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    row.map(row_to_hold).transpose()
}

pub(super) async fn put(
    pool: &PgPool,
    hold: &PrintPreflightHold,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    lock_order_and_apparatuses_tx(&mut tx, &hold.order_id, &[hold.apparatus.as_str()]).await?;
    let current = sqlx::query_scalar::<_, String>(
        "SELECT state FROM mini_queue_states WHERE canonical_apparatus_id = $1 AND order_id = $2 FOR UPDATE",
    ).bind(&hold.apparatus).bind(&hold.order_id).fetch_optional(&mut *tx).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let busy = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM mini_queue_states
         WHERE canonical_apparatus_id = $1 AND state IN ('in_progress', 'print_preflight'))",
    )
    .bind(&hold.apparatus)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if busy
        || current != hold.previous_queue_state
        || current.as_deref().is_some_and(|state| state != "pending")
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    sqlx::query(
        "INSERT INTO mini_print_preflight_holds (
            hold_id, idempotency_key, order_id, canonical_apparatus_id,
            stage_node_id, status, actor_role, actor_ref, actor_display_name,
            created_at_unix, updated_at_unix, expires_at_unix, previous_queue_state
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
         ON CONFLICT (hold_id) DO UPDATE SET
            status = EXCLUDED.status,
            actor_role = EXCLUDED.actor_role,
            actor_ref = EXCLUDED.actor_ref,
            actor_display_name = EXCLUDED.actor_display_name,
            updated_at_unix = EXCLUDED.updated_at_unix,
            expires_at_unix = EXCLUDED.expires_at_unix",
    )
    .bind(&hold.hold_id)
    .bind(&hold.idempotency_key)
    .bind(&hold.order_id)
    .bind(&hold.apparatus)
    .bind(&hold.stage_node_id)
    .bind(hold.status.as_str())
    .bind(&hold.actor.role)
    .bind(&hold.actor.ref_)
    .bind(&hold.actor.display_name)
    .bind(hold.created_at_unix)
    .bind(hold.updated_at_unix)
    .bind(hold.expires_at_unix)
    .bind(&hold.previous_queue_state)
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    persist_queue_status(&mut tx, hold).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn update(
    pool: &PgPool,
    hold: &PrintPreflightHold,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    lock_order_and_apparatuses_tx(&mut tx, &hold.order_id, &[hold.apparatus.as_str()]).await?;
    let result = sqlx::query(
        "UPDATE mini_print_preflight_holds
         SET status = $2, actor_role = $3, actor_ref = $4,
             actor_display_name = $5, updated_at_unix = $6,
             expires_at_unix = $7
         WHERE hold_id = $1 AND status IN ('held', 'running', 'passed')",
    )
    .bind(&hold.hold_id)
    .bind(hold.status.as_str())
    .bind(&hold.actor.role)
    .bind(&hold.actor.ref_)
    .bind(&hold.actor.display_name)
    .bind(hold.updated_at_unix)
    .bind(hold.expires_at_unix)
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if result.rows_affected() == 0 {
        return Err(ProductionMapError::PrintPreflightNotFound);
    }
    persist_queue_status(&mut tx, hold).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn consume_print_preflight_hold_tx(
    tx: &mut Transaction<'_, Postgres>,
    hold_id: &str,
    order_id: &str,
    apparatus: &str,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    lock_order_and_apparatuses_tx(tx, order_id, &[apparatus]).await?;
    let result = sqlx::query(
        "UPDATE mini_print_preflight_holds
         SET status = 'consumed', actor_role = $4, actor_ref = $5,
             actor_display_name = $6,
             updated_at_unix = EXTRACT(EPOCH FROM now())::BIGINT
         WHERE hold_id = $1 AND order_id = $2 AND canonical_apparatus_id = $3
           AND status = 'passed'",
    )
    .bind(hold_id.trim())
    .bind(order_id.trim())
    .bind(apparatus.trim())
    .bind(&actor.role)
    .bind(&actor.ref_)
    .bind(&actor.display_name)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if result.rows_affected() == 0 {
        return Err(ProductionMapError::PrintPreflightNotReady);
    }
    Ok(())
}

fn row_to_hold(row: PrintPreflightHoldRow) -> Result<PrintPreflightHold, ProductionMapError> {
    let status = PrintPreflightStatus::parse(&row.status).ok_or(ProductionMapError::StoreFailed)?;
    Ok(PrintPreflightHold {
        hold_id: row.hold_id,
        idempotency_key: row.idempotency_key,
        order_id: row.order_id,
        apparatus: row.canonical_apparatus_id,
        stage_node_id: row.stage_node_id,
        status,
        actor: QueueActionActor {
            role: row.actor_role,
            ref_: row.actor_ref,
            display_name: row.actor_display_name,
        },
        created_at_unix: row.created_at_unix,
        updated_at_unix: row.updated_at_unix,
        previous_queue_state: row.previous_queue_state,
        expires_at_unix: row.expires_at_unix,
    })
}

// Write the same queue and persisted order projection used by ordinary actions.
// Trial metadata is only used to validate the result and restore the prior state.
async fn persist_queue_status(
    tx: &mut Transaction<'_, Postgres>,
    hold: &PrintPreflightHold,
) -> Result<(), ProductionMapError> {
    let state = if hold.status.reserves_apparatus() {
        Some("print_preflight")
    } else {
        hold.previous_queue_state.as_deref()
    };
    if let Some(state) = state {
        sqlx::query(
            "INSERT INTO mini_queue_states
                (apparatus, canonical_apparatus_id, order_id, state, updated_at)
             VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id = $1), $1), $1, $2, $3, now())
             ON CONFLICT (canonical_apparatus_id, order_id)
             DO UPDATE SET state = EXCLUDED.state, updated_at = now()",
        )
        .bind(&hold.apparatus)
        .bind(&hold.order_id)
        .bind(state)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    } else {
        sqlx::query("DELETE FROM mini_queue_states WHERE canonical_apparatus_id = $1 AND order_id = $2 AND state = 'print_preflight'")
            .bind(&hold.apparatus).bind(&hold.order_id)
            .execute(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    }
    super::lifecycle::refresh_production_order_lifecycle_tx(
        tx,
        &hold.order_id,
        &hold.actor,
        &hold.hold_id,
        "print_preflight",
    )
    .await
}
