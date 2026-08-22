use std::collections::BTreeMap;

use sqlx::{Postgres, Transaction};

use crate::core::production_map::{
    reject_training_order_id, queue_state::ApparatusQueueAction,
    ApparatusQueueActionEvent, ProductionMapError,
};

pub(super) async fn lock_apparatus_queue_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
) -> Result<(), ProductionMapError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!(
            "mini-rs-erp:queue-apparatus:{}",
            apparatus.trim().to_lowercase()
        ))
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn lock_order_control_tx(
    tx: &mut Transaction<'_, Postgres>,
    order_id: &str,
) -> Result<Option<(String, Option<String>)>, ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    reject_training_order_id(order_id)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("mini-rs-erp:order-control:{order_id}"))
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT state, freeze_request_id
         FROM mini_order_control_states
         WHERE order_id = $1
         FOR UPDATE",
    )
    .bind(order_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn queue_event_already_applied_tx(
    tx: &mut Transaction<'_, Postgres>,
    event: &ApparatusQueueActionEvent,
) -> Result<bool, ProductionMapError> {
    let event_id = event.event_id.trim();
    if event_id.is_empty() {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("mini-rs-erp:queue-event:{event_id}"))
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let existing = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT apparatus, order_id, action, from_state, to_state
         FROM mini_queue_action_events
         WHERE event_id = $1
         FOR UPDATE",
    )
    .bind(event.event_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some((apparatus, order_id, action, from_state, to_state)) = existing else {
        return Ok(false);
    };
    let matches = apparatus.trim() == event.apparatus.trim()
        && order_id.trim() == event.order_id.trim()
        && action == queue_action_as_str(event.action)
        && from_state == event.from_state.as_str()
        && to_state == event.to_state.as_str();
    if !matches {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    Ok(true)
}

pub(super) async fn put_queue_state_for_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
    states: &BTreeMap<String, String>,
    event: &ApparatusQueueActionEvent,
) -> Result<(), ProductionMapError> {
    ensure_queue_state_for_event_tx(tx, apparatus, event).await?;
    let desired = states
        .get(event.order_id.trim())
        .map(|state| state.trim())
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    if desired != event.to_state.as_str() {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    sqlx::query(
        "INSERT INTO mini_queue_states (apparatus, order_id, state, updated_at)
         VALUES ($1, $2, $3, now())
         ON CONFLICT (apparatus, order_id) DO UPDATE SET
           state = excluded.state,
           updated_at = excluded.updated_at",
    )
    .bind(apparatus.trim())
    .bind(event.order_id.trim())
    .bind(desired)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn ensure_queue_state_for_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
    event: &ApparatusQueueActionEvent,
) -> Result<(), ProductionMapError> {
    let current = sqlx::query_scalar::<_, String>(
        "SELECT state
         FROM mini_queue_states
         WHERE apparatus = $1 AND order_id = $2
         FOR UPDATE",
    )
    .bind(apparatus.trim())
    .bind(event.order_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .unwrap_or_else(|| "pending".to_string());
    reject_training_order_id(&event.order_id)?;
    if current.trim() != event.from_state.as_str() {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    Ok(())
}

pub(super) async fn save_sequence_updates_for_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    sequence_updates: &BTreeMap<String, Vec<String>>,
    event: &ApparatusQueueActionEvent,
) -> Result<(), ProductionMapError> {
    if sequence_updates.is_empty() {
        return Ok(());
    }
    let is_freeze = event.action == ApparatusQueueAction::Freeze;
    let is_unfreeze = event
        .payload_json
        .get("admin_unfreeze")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let mut frozen_order_ids = if is_freeze {
        sqlx::query_scalar::<_, String>(
            "SELECT order_id
             FROM mini_order_control_states
             WHERE state = 'frozen'",
        )
        .fetch_all(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
    } else {
        Vec::new()
    };
    if is_freeze {
        frozen_order_ids.push(event.order_id.trim().to_string());
    }
    let current_frozen = |order_id: &str| {
        frozen_order_ids
            .iter()
            .any(|frozen| frozen.trim() == order_id.trim())
    };

    for (apparatus, incoming_order_ids) in sequence_updates {
        let current = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT order_ids
             FROM mini_queue_sequences
             WHERE apparatus = $1
             FOR UPDATE",
        )
        .bind(apparatus.trim())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut order_ids = current
            .map(|payload| {
                serde_json::from_value::<Vec<String>>(payload)
                    .map_err(|_| ProductionMapError::StoreFailed)
            })
            .transpose()?
            .unwrap_or_else(|| incoming_order_ids.clone());
        if is_freeze {
            order_ids.retain(|order_id| !current_frozen(order_id));
        } else if is_unfreeze {
            order_ids.retain(|order_id| order_id.trim() != event.order_id.trim());
            if apparatus.trim().eq_ignore_ascii_case(event.apparatus.trim()) {
                order_ids.push(event.order_id.trim().to_string());
            }
        }
        super::catalog_helpers::save_apparatus_sequence_tx(tx, apparatus, &order_ids).await?;
    }
    Ok(())
}

pub(super) async fn put_queue_states_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
    states: BTreeMap<String, String>,
) -> Result<(), ProductionMapError> {
    sqlx::query("DELETE FROM mini_queue_states WHERE apparatus = $1")
        .bind(apparatus)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    for (order_id, state) in states {
        reject_training_order_id(&order_id)?;
        sqlx::query(
            "INSERT INTO mini_queue_states (apparatus, order_id, state, updated_at)
             VALUES ($1, $2, $3, now())",
        )
        .bind(apparatus)
        .bind(order_id.trim())
        .bind(state.trim())
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}

pub(super) async fn insert_queue_action_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    event: &ApparatusQueueActionEvent,
) -> Result<(), ProductionMapError> {
    reject_training_order_id(&event.order_id)?;
    if queue_event_already_applied_tx(tx, event).await? {
        return Ok(());
    }
    if event.action == ApparatusQueueAction::Complete
        && event
            .payload_json
            .get("completion_request")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    {
        let pending_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                 SELECT 1
                 FROM mini_queue_action_events
                 WHERE lower(apparatus) = lower($1)
                   AND order_id = $2
                   AND event_id <> $3
                   AND action = 'complete'
                   AND payload_json->>'completion_request' = 'true'
                   AND COALESCE(payload_json->>'completion_request_status', 'pending') = 'pending'
             )",
        )
        .bind(event.apparatus.trim())
        .bind(event.order_id.trim())
        .bind(event.event_id.trim())
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if pending_exists {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
    }
    sqlx::query(
        "INSERT INTO mini_queue_action_events
            (event_id, apparatus, order_id, action, from_state, to_state, policy,
             actor_role, actor_ref, actor_display_name, assigned_apparatus, payload_json, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now())
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(event.event_id.trim())
    .bind(event.apparatus.trim())
    .bind(event.order_id.trim())
    .bind(match event.action {
        crate::core::production_map::queue_state::ApparatusQueueAction::Start => "start",
        crate::core::production_map::queue_state::ApparatusQueueAction::Pause => "pause",
        crate::core::production_map::queue_state::ApparatusQueueAction::Freeze => "freeze",
        crate::core::production_map::queue_state::ApparatusQueueAction::DetachRoll => {
            "detach_roll"
        }
        crate::core::production_map::queue_state::ApparatusQueueAction::Resume => "resume",
        crate::core::production_map::queue_state::ApparatusQueueAction::RollComplete => {
            "roll_complete"
        }
        crate::core::production_map::queue_state::ApparatusQueueAction::Complete => "complete",
    })
    .bind(event.from_state.as_str())
    .bind(event.to_state.as_str())
    .bind(event.policy.as_str())
    .bind(event.actor.role.trim())
    .bind(event.actor.ref_.trim())
    .bind(event.actor.display_name.trim())
    .bind(
        serde_json::to_value(&event.assigned_apparatus)
            .map_err(|_| ProductionMapError::StoreFailed)?,
    )
    .bind(&event.payload_json)
    .execute(&mut **tx)
    .await
    .map_err(|error| {
        let is_unique_violation = matches!(
            &error,
            sqlx::Error::Database(database) if database.code().as_deref() == Some("23505")
        );
        if is_unique_violation {
            ProductionMapError::QueueActionNotAllowed
        } else {
            ProductionMapError::StoreFailed
        }
    })?;
    Ok(())
}

pub(super) fn queue_action_from_str(
    value: &str,
) -> Option<crate::core::production_map::queue_state::ApparatusQueueAction> {
    match value.trim().to_ascii_lowercase().as_str() {
        "start" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Start),
        "pause" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Pause),
        "freeze" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Freeze),
        "detach_roll" => Some(
            crate::core::production_map::queue_state::ApparatusQueueAction::DetachRoll,
        ),
        "resume" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Resume),
        "roll_complete" => Some(
            crate::core::production_map::queue_state::ApparatusQueueAction::RollComplete,
        ),
        "complete" => {
            Some(crate::core::production_map::queue_state::ApparatusQueueAction::Complete)
        }
        _ => None,
    }
}

pub(super) fn queue_action_as_str(
    action: crate::core::production_map::queue_state::ApparatusQueueAction,
) -> &'static str {
    match action {
        crate::core::production_map::queue_state::ApparatusQueueAction::Start => "start",
        crate::core::production_map::queue_state::ApparatusQueueAction::Pause => "pause",
        crate::core::production_map::queue_state::ApparatusQueueAction::Freeze => "freeze",
        crate::core::production_map::queue_state::ApparatusQueueAction::DetachRoll => {
            "detach_roll"
        }
        crate::core::production_map::queue_state::ApparatusQueueAction::Resume => "resume",
        crate::core::production_map::queue_state::ApparatusQueueAction::RollComplete => {
            "roll_complete"
        }
        crate::core::production_map::queue_state::ApparatusQueueAction::Complete => "complete",
    }
}
