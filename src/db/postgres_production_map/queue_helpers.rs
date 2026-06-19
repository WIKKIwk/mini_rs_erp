use std::collections::BTreeMap;

use sqlx::{Postgres, Transaction};

use crate::core::production_map::{ApparatusQueueActionEvent, ProductionMapError};

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
    sqlx::query(
        "INSERT INTO mini_queue_action_events
            (event_id, apparatus, order_id, action, from_state, to_state, policy,
             actor_role, actor_ref, actor_display_name, assigned_apparatus, payload_json, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, now())",
    )
    .bind(event.event_id.trim())
    .bind(event.apparatus.trim())
    .bind(event.order_id.trim())
    .bind(match event.action {
        crate::core::production_map::queue_state::ApparatusQueueAction::Start => "start",
        crate::core::production_map::queue_state::ApparatusQueueAction::Pause => "pause",
        crate::core::production_map::queue_state::ApparatusQueueAction::Resume => "resume",
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
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) fn queue_action_from_str(
    value: &str,
) -> Option<crate::core::production_map::queue_state::ApparatusQueueAction> {
    match value.trim().to_ascii_lowercase().as_str() {
        "start" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Start),
        "pause" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Pause),
        "resume" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Resume),
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
        crate::core::production_map::queue_state::ApparatusQueueAction::Resume => "resume",
        crate::core::production_map::queue_state::ApparatusQueueAction::Complete => "complete",
    }
}
