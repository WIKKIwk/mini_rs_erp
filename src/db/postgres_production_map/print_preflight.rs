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
    revision: i64,
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
    created_at_unix, updated_at_unix, expires_at_unix, previous_queue_state, revision";

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
    put_with_event(pool, hold).await.map(|_| ())
}

pub(super) async fn put_with_event(
    pool: &PgPool,
    hold: &PrintPreflightHold,
) -> Result<String, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    lock_order_and_apparatuses_tx(&mut tx, &hold.order_id, &[hold.apparatus.as_str()]).await?;
    let event_id = tag_preflight_notification(&mut tx, hold).await?;
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
    let result = sqlx::query(
        "INSERT INTO mini_print_preflight_holds (
            hold_id, idempotency_key, order_id, canonical_apparatus_id,
            stage_node_id, status, actor_role, actor_ref, actor_display_name,
            created_at_unix, updated_at_unix, expires_at_unix, previous_queue_state, revision
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
         ON CONFLICT (hold_id) DO NOTHING",
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
    .bind(hold.revision)
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if result.rows_affected() != 1 {
        return Err(ProductionMapError::PrintPreflightActive);
    }
    persist_queue_status(&mut tx, hold).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(event_id)
}

pub(super) async fn update(
    pool: &PgPool,
    hold: &PrintPreflightHold,
) -> Result<(), ProductionMapError> {
    update_with_event(pool, hold).await.map(|_| ())
}

pub(super) async fn update_with_event(
    pool: &PgPool,
    hold: &PrintPreflightHold,
) -> Result<String, ProductionMapError> {
    update_with_event_checked(pool, hold, false).await
}

pub(super) async fn reopen_with_event(
    pool: &PgPool,
    hold: &PrintPreflightHold,
) -> Result<String, ProductionMapError> {
    update_with_event_checked(pool, hold, true).await
}

async fn update_with_event_checked(
    pool: &PgPool,
    hold: &PrintPreflightHold,
    reopening: bool,
) -> Result<String, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    lock_order_and_apparatuses_tx(&mut tx, &hold.order_id, &[hold.apparatus.as_str()]).await?;
    if reopening {
        let ready: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1 FROM mini_print_preflight_holds h
                JOIN mini_queue_states q ON q.order_id = h.order_id
                    AND q.canonical_apparatus_id = h.canonical_apparatus_id
                LEFT JOIN mini_order_control_states c ON c.order_id = h.order_id
                WHERE h.hold_id = $1 AND h.order_id = $2
                    AND h.canonical_apparatus_id = $3 AND h.stage_node_id = $4
                    AND h.status = 'passed' AND q.state = 'print_preflight'
                    AND (c.state IS NULL OR c.state = 'active')
            )",
        )
        .bind(&hold.hold_id)
        .bind(&hold.order_id)
        .bind(&hold.apparatus)
        .bind(&hold.stage_node_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if hold.status != PrintPreflightStatus::Running || !ready {
            return Err(ProductionMapError::PrintPreflightNotReady);
        }
    }
    let event_id = tag_preflight_notification(&mut tx, hold).await?;
    let result = sqlx::query(
        "UPDATE mini_print_preflight_holds
         SET status = $2, actor_role = $3, actor_ref = $4,
             actor_display_name = $5, updated_at_unix = $6,
             expires_at_unix = $7, revision = $8
         WHERE hold_id = $1 AND status IN ('held', 'running', 'passed')
             AND revision = $8 - 1 AND $8 > 0
             AND order_id = $9 AND canonical_apparatus_id = $10 AND stage_node_id = $11",
    )
    .bind(&hold.hold_id)
    .bind(hold.status.as_str())
    .bind(&hold.actor.role)
    .bind(&hold.actor.ref_)
    .bind(&hold.actor.display_name)
    .bind(hold.updated_at_unix)
    .bind(hold.expires_at_unix)
    .bind(hold.revision)
    .bind(&hold.order_id)
    .bind(&hold.apparatus)
    .bind(&hold.stage_node_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if result.rows_affected() == 0 {
        return Err(ProductionMapError::PrintPreflightNotReady);
    }
    persist_queue_status(&mut tx, hold).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(event_id)
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
             updated_at_unix = EXTRACT(EPOCH FROM now())::BIGINT, revision = revision + 1
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

pub(super) async fn cancel_print_preflight_hold_tx(
    tx: &mut Transaction<'_, Postgres>,
    hold_id: &str,
    order_id: &str,
    apparatus: &str,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    lock_order_and_apparatuses_tx(tx, order_id, &[apparatus]).await?;
    let result = sqlx::query(
        "UPDATE mini_print_preflight_holds
         SET status = 'cancelled', actor_role = $4, actor_ref = $5,
             actor_display_name = $6,
             updated_at_unix = EXTRACT(EPOCH FROM now())::BIGINT, revision = revision + 1
         WHERE hold_id = $1 AND order_id = $2 AND canonical_apparatus_id = $3
           AND status IN ('held', 'running', 'passed')",
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
        revision: row.revision,
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

// Only these two dedicated transactions have a colour-only dependency set.
// Queue actions consuming a hold use the conservative generic notification.
async fn tag_preflight_notification(
    tx: &mut Transaction<'_, Postgres>,
    hold: &PrintPreflightHold,
) -> Result<String, ProductionMapError> {
    let event_id = format!("{:032x}", rand::random::<u128>());
    let payload = serde_json::json!({"kind":"preflight", "order_id":hold.order_id,
        "apparatus":hold.apparatus, "event_id":event_id})
    .to_string();
    sqlx::query("SELECT set_config('mini.production_live', $1, true)")
        .bind(payload)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(event_id)
}
