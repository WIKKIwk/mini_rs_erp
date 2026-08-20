use std::collections::BTreeMap;

use sqlx::{Postgres, Transaction};

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    ApparatusQueueActionEvent, ProductionMapError, queue_state::ApparatusQueueAction,
};

use super::transaction_locks::lock_apparatus_tx;

pub(super) async fn queue_action_event_replay_tx(
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
    let apparatus_id = ApparatusId::new(event.apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::ScheduleInputInvalid)?;
    let assigned_apparatus = normalized_assigned_apparatus(event)?;
    let existing = sqlx::query_as::<
        _,
        (
            Option<String>,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            serde_json::Value,
            serde_json::Value,
        ),
    >(
        "SELECT canonical_apparatus_id, order_id, action, from_state, to_state, policy,
                actor_role, actor_ref, actor_display_name, assigned_apparatus, payload_json
         FROM mini_queue_action_events
         WHERE event_id = $1
         FOR UPDATE",
    )
    .bind(event_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some((
        existing_apparatus,
        existing_order_id,
        existing_action,
        existing_from_state,
        existing_to_state,
        existing_policy,
        existing_actor_role,
        existing_actor_ref,
        existing_actor_display_name,
        existing_assigned_apparatus,
        existing_payload,
    )) = existing
    else {
        return Ok(false);
    };

    let same_request = queue_event_identity_matches(
        existing_apparatus.as_deref(),
        &existing_order_id,
        &existing_action,
        &existing_from_state,
        &existing_to_state,
        &existing_policy,
        &existing_actor_role,
        &existing_actor_ref,
        &existing_actor_display_name,
        &existing_assigned_apparatus,
        &existing_payload,
        &apparatus_id,
        event,
        &assigned_apparatus,
    )?;
    if !same_request {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    Ok(true)
}

pub(super) async fn validate_queue_action_event_transition_tx(
    tx: &mut Transaction<'_, Postgres>,
    event: &ApparatusQueueActionEvent,
) -> Result<(), ProductionMapError> {
    let apparatus_id = ApparatusId::new(event.apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::ScheduleInputInvalid)?;
    let current_state = sqlx::query_scalar::<_, String>(
        "SELECT state
         FROM mini_queue_states
         WHERE canonical_apparatus_id = $1 AND order_id = $2
         FOR UPDATE",
    )
    .bind(apparatus_id.as_str())
    .bind(event.order_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .unwrap_or_else(|| "pending".to_string());
    if current_state.trim() != event.from_state.as_str() {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    Ok(())
}

pub(super) async fn put_queue_states_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
    states: BTreeMap<String, String>,
) -> Result<(), ProductionMapError> {
    let apparatus_id = lock_apparatus_tx(tx, apparatus).await?;
    sqlx::query("DELETE FROM mini_queue_states WHERE canonical_apparatus_id = $1")
        .bind(apparatus_id.as_str())
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    for (order_id, state) in states {
        sqlx::query(
            "INSERT INTO mini_queue_states
                (apparatus, canonical_apparatus_id, order_id, state, updated_at)
             VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id = $1), $1), $1, $2, $3, now())",
        )
        .bind(apparatus_id.as_str())
        .bind(order_id.trim())
        .bind(state.trim())
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}

pub(super) async fn put_queue_action_state_tx(
    tx: &mut Transaction<'_, Postgres>,
    event: &ApparatusQueueActionEvent,
) -> Result<(), ProductionMapError> {
    let apparatus_id = lock_apparatus_tx(tx, &event.apparatus).await?;
    let updated = sqlx::query(
        "UPDATE mini_queue_states
         SET state = $3, updated_at = now()
         WHERE canonical_apparatus_id = $1 AND order_id = $2",
    )
    .bind(apparatus_id.as_str())
    .bind(event.order_id.trim())
    .bind(event.to_state.as_str())
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if updated.rows_affected() == 0 {
        sqlx::query(
            "INSERT INTO mini_queue_states
                (apparatus, canonical_apparatus_id, order_id, state, updated_at)
             VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id = $1), $1), $1, $2, $3, now())",
        )
        .bind(apparatus_id.as_str())
        .bind(event.order_id.trim())
        .bind(event.to_state.as_str())
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
    let apparatus_id = ApparatusId::new(event.apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::ScheduleInputInvalid)?;
    let assigned_apparatus = normalized_assigned_apparatus(event)?;
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
                 WHERE canonical_apparatus_id = $1
                   AND order_id = $2
                   AND event_id <> $3
                   AND action = 'complete'
                   AND payload_json->>'completion_request' = 'true'
                   AND COALESCE(payload_json->>'completion_request_status', 'pending') = 'pending'
             )",
        )
        .bind(apparatus_id.as_str())
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
            (event_id, apparatus, canonical_apparatus_id, order_id, action, from_state, to_state, policy,
             actor_role, actor_ref, actor_display_name, assigned_apparatus, payload_json, created_at)
         VALUES ($1, COALESCE((SELECT name FROM mini_apparatus WHERE id = $2), $2), $2, $3, $4, $5, $6, $7,
                 $8, $9, $10, $11, $12, now())",
    )
    .bind(event.event_id.trim())
    .bind(apparatus_id.as_str())
    .bind(event.order_id.trim())
    .bind(queue_action_as_str(event.action))
    .bind(event.from_state.as_str())
    .bind(event.to_state.as_str())
    .bind(event.policy.as_str())
    .bind(event.actor.role.trim())
    .bind(event.actor.ref_.trim())
    .bind(event.actor.display_name.trim())
    .bind(
        serde_json::to_value(assigned_apparatus)
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

fn normalized_assigned_apparatus(
    event: &ApparatusQueueActionEvent,
) -> Result<Vec<String>, ProductionMapError> {
    event
        .assigned_apparatus
        .iter()
        .map(|value| {
            ApparatusId::new(value.trim().to_string())
                .map(|id| id.to_string())
                .map_err(|_| ProductionMapError::ScheduleInputInvalid)
        })
        .collect::<Result<Vec<_>, _>>()
}

fn queue_event_identity_matches(
    existing_apparatus: Option<&str>,
    existing_order_id: &str,
    existing_action: &str,
    existing_from_state: &str,
    existing_to_state: &str,
    existing_policy: &str,
    existing_actor_role: &str,
    existing_actor_ref: &str,
    existing_actor_display_name: &str,
    existing_assigned_apparatus: &serde_json::Value,
    existing_payload: &serde_json::Value,
    apparatus_id: &ApparatusId,
    event: &ApparatusQueueActionEvent,
    assigned_apparatus: &[String],
) -> Result<bool, ProductionMapError> {
    let assigned_apparatus =
        serde_json::to_value(assigned_apparatus).map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(existing_apparatus == Some(apparatus_id.as_str())
        && existing_order_id.trim() == event.order_id.trim()
        && existing_action == queue_action_as_str(event.action)
        && existing_from_state == event.from_state.as_str()
        && existing_to_state == event.to_state.as_str()
        && existing_policy == event.policy.as_str()
        && existing_actor_role.trim() == event.actor.role.trim()
        && existing_actor_ref.trim() == event.actor.ref_.trim()
        && existing_actor_display_name.trim() == event.actor.display_name.trim()
        && existing_assigned_apparatus == &assigned_apparatus
        && existing_payload == &event.payload_json)
}

pub(super) fn queue_action_from_str(
    value: &str,
) -> Option<crate::core::production_map::queue_state::ApparatusQueueAction> {
    match value.trim().to_ascii_lowercase().as_str() {
        "start" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Start),
        "pause" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Pause),
        "freeze" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Freeze),
        "detach_roll" => {
            Some(crate::core::production_map::queue_state::ApparatusQueueAction::DetachRoll)
        }
        "resume" => Some(crate::core::production_map::queue_state::ApparatusQueueAction::Resume),
        "roll_complete" => {
            Some(crate::core::production_map::queue_state::ApparatusQueueAction::RollComplete)
        }
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
        crate::core::production_map::queue_state::ApparatusQueueAction::DetachRoll => "detach_roll",
        crate::core::production_map::queue_state::ApparatusQueueAction::Resume => "resume",
        crate::core::production_map::queue_state::ApparatusQueueAction::RollComplete => {
            "roll_complete"
        }
        crate::core::production_map::queue_state::ApparatusQueueAction::Complete => "complete",
    }
}

#[cfg(test)]
mod tests {
    use super::queue_event_identity_matches;
    use crate::core::apparatus_standard::ApparatusId;
    use crate::core::production_map::{
        ApparatusQueueActionEvent, ApparatusQueuePolicy, QueueActionActor,
    };
    use crate::core::production_map::queue_state::{
        ApparatusQueueAction, ApparatusQueueOrderState,
    };

    #[test]
    fn queue_event_replay_requires_the_original_payload() {
        let event = ApparatusQueueActionEvent {
            event_id: "event-1".to_string(),
            apparatus: "apparatus:test:a".to_string(),
            order_id: "zakaz-1".to_string(),
            action: ApparatusQueueAction::Start,
            from_state: ApparatusQueueOrderState::Pending,
            to_state: ApparatusQueueOrderState::InProgress,
            policy: ApparatusQueuePolicy::FreePick,
            actor: QueueActionActor {
                role: "operator".to_string(),
                ref_: "worker-1".to_string(),
                display_name: "Worker".to_string(),
            },
            assigned_apparatus: vec!["apparatus:test:a".to_string()],
            payload_json: serde_json::json!({"request": "one"}),
        };
        let apparatus_id = ApparatusId::new("apparatus:test:a".to_string()).unwrap();
        let assigned = vec!["apparatus:test:a".to_string()];
        let matching = queue_event_identity_matches(
            Some("apparatus:test:a"),
            "zakaz-1",
            "start",
            "pending",
            "in_progress",
            "free_pick",
            "operator",
            "worker-1",
            "Worker",
            &serde_json::json!(["apparatus:test:a"]),
            &event.payload_json,
            &apparatus_id,
            &event,
            &assigned,
        )
        .unwrap();
        assert!(matching);

        let changed = queue_event_identity_matches(
            Some("apparatus:test:a"),
            "zakaz-1",
            "start",
            "pending",
            "in_progress",
            "free_pick",
            "operator",
            "worker-1",
            "Worker",
            &serde_json::json!(["apparatus:test:a"]),
            &serde_json::json!({"request": "two"}),
            &apparatus_id,
            &event,
            &assigned,
        )
        .unwrap();
        assert!(!changed);
    }
}
