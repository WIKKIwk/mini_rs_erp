use super::*;
use crate::core::production_map::{queue_state::ApparatusQueueAction, re_pause as domain};
use sqlx::{Postgres, Transaction};

async fn current_session_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
    order_id: &str,
) -> Result<Option<OrderRunSession>, ProductionMapError> {
    sqlx::query("SELECT session_id FROM mini_order_run_sessions WHERE order_id=$1 AND canonical_apparatus_id=$2 FOR UPDATE")
        .bind(order_id).bind(apparatus).fetch_all(&mut **tx).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(stage_execution::sessions_tx(tx, order_id)
        .await?
        .into_iter()
        .filter(|session| session.apparatus == apparatus && session.status.is_open())
        .max_by_key(|session| (session.started_at_unix, session.session_id.clone())))
}

pub(super) async fn validate_resume_checkpoint_tx(
    tx: &mut Transaction<'_, Postgres>,
    write: &QueueActionProgressWrite,
) -> Result<(), ProductionMapError> {
    if let Some(expected) = write
        .event
        .payload_json
        .get("resume_pause_expected_session")
    {
        if write.event.action != ApparatusQueueAction::Resume {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let control: Option<(String, Option<serde_json::Value>)> = sqlx::query_as(
            "SELECT state,early_close FROM mini_order_control_states WHERE order_id=$1 FOR UPDATE",
        )
        .bind(&write.event.order_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if control.is_some_and(|(state, early_close)| state != "active" || early_close.is_some()) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let current = current_session_tx(tx, &write.apparatus, &write.event.order_id)
            .await?
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        if serde_json::to_value(&current).map_err(|_| ProductionMapError::StoreFailed)? != *expected
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
    }
    Ok(())
}

pub(super) async fn re_pause(
    store: &PostgresProductionMapStore,
    apparatus: &str,
    order_id: &str,
    resume_id: &str,
    actor: &QueueActionActor,
    assigned_apparatus: &[String],
) -> Result<(), ProductionMapError> {
    if actor.role != "aparatchi"
        || actor.ref_.trim().is_empty()
        || !assigned_apparatus.iter().any(|id| id == apparatus)
        || order_id.starts_with("training-")
        || resume_id.is_empty()
        || resume_id.len() > 512
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let mut tx = store
        .pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    transaction_locks::lock_order_and_apparatuses_tx(&mut tx, order_id, &[apparatus]).await?;
    let receipt: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT order_id,canonical_apparatus_id,actor_role,actor_ref FROM mini_queue_action_events WHERE event_id=$1",
    ).bind(format!("re-pause:{resume_id}")).fetch_optional(&mut *tx).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    if let Some(receipt) = receipt {
        if receipt
            != (
                order_id.into(),
                apparatus.into(),
                actor.role.clone(),
                actor.ref_.clone(),
            )
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        // A replay acknowledges the receipt without restoring stale state.
        return Ok(());
    }
    let control: Option<String> = sqlx::query_scalar(
        "SELECT state FROM mini_order_control_states WHERE order_id=$1 FOR UPDATE",
    )
    .bind(order_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if control.as_deref().is_some_and(|state| state != "active") {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let state: Option<String> = sqlx::query_scalar(
        "SELECT state FROM mini_queue_states WHERE order_id=$1 AND canonical_apparatus_id=$2 FOR UPDATE",
    ).bind(order_id).bind(apparatus).fetch_optional(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if state.as_deref() != Some("in_progress") {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let current = current_session_tx(&mut tx, apparatus, order_id)
        .await?
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    let latest: Option<(String, String)> = sqlx::query_as(
        "SELECT event_id,policy FROM mini_queue_action_events WHERE order_id=$1 AND canonical_apparatus_id=$2 ORDER BY id DESC LIMIT 1",
    ).bind(order_id).bind(apparatus).fetch_optional(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let policy = latest
        .filter(|(id, _)| id == resume_id)
        .and_then(|(_, policy)| crate::core::production_map::ApparatusQueuePolicy::parse(&policy))
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    let states = sqlx::query_as::<_, (String, String)>(
        "SELECT order_id,state FROM mini_queue_states WHERE canonical_apparatus_id=$1",
    )
    .bind(apparatus)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .into_iter()
    .collect();
    let write = domain::cancellation_write(
        &current,
        resume_id,
        actor,
        assigned_apparatus,
        states,
        policy,
    )?;
    apply_queue_progress_tx(&mut tx, &write).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}
