use std::collections::BTreeMap;

use sqlx::{Postgres, Transaction};

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    ApparatusQueueActionEvent, OrderRunInputSourceKind, OrderRunInputStatus, OrderRunSession,
    ProductionMapError, order_run_input_links_from_payload, queue_state::ApparatusQueueAction,
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

    let existing = StoredQueueEventIdentity {
        apparatus: existing_apparatus.as_deref(),
        order_id: &existing_order_id,
        action: &existing_action,
        from_state: &existing_from_state,
        to_state: &existing_to_state,
        policy: &existing_policy,
        actor_role: &existing_actor_role,
        actor_ref: &existing_actor_ref,
        actor_display_name: &existing_actor_display_name,
        assigned_apparatus: &existing_assigned_apparatus,
        payload: &existing_payload,
    };
    let same_request = queue_event_identity_matches(
        &existing,
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

pub(super) async fn validate_merge_session_transition_tx(
    tx: &mut Transaction<'_, Postgres>,
    event: &ApparatusQueueActionEvent,
    proposed_session: Option<&OrderRunSession>,
) -> Result<(), ProductionMapError> {
    if event.action != ApparatusQueueAction::Merge {
        return Ok(());
    }
    let proposed_session = proposed_session.ok_or(ProductionMapError::MergeInputNotAccepted)?;
    let apparatus_id = ApparatusId::new(event.apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::MergeInputNotAccepted)?;
    let current = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT status, payload_json
         FROM mini_order_run_sessions
         WHERE session_id = $1
           AND order_id = $2
           AND canonical_apparatus_id = $3
         FOR UPDATE",
    )
    .bind(proposed_session.session_id.trim())
    .bind(event.order_id.trim())
    .bind(apparatus_id.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some((status, current_payload)) = current else {
        return Err(ProductionMapError::MergeInputNotAccepted);
    };
    if status.trim() != "active"
        || !merge_session_transition_matches(&current_payload, proposed_session)
    {
        return Err(ProductionMapError::MergeInputNotAccepted);
    }
    let proposed_links = order_run_input_links_from_payload(&proposed_session.payload_json)
        .map_err(|_| ProductionMapError::MergeInputNotAccepted)?;
    let next_input = proposed_links
        .iter()
        .find(|link| link.status == OrderRunInputStatus::InUse)
        .ok_or(ProductionMapError::MergeInputNotAccepted)?;
    let candidate = match next_input.source_kind {
        OrderRunInputSourceKind::ProgressBatch => {
            sqlx::query_as::<_, (String, String)>(
                "SELECT order_id, wip_status
                 FROM mini_progress_batches
                 WHERE batch_id = $1
                 FOR UPDATE",
            )
            .bind(next_input.input_batch_id.trim())
            .fetch_optional(&mut **tx)
            .await
        }
        OrderRunInputSourceKind::OpeningWip => {
            sqlx::query_as::<_, (String, String)>(
                "SELECT order_id, wip_status
                 FROM mini_opening_wip_batches
                 WHERE batch_id = $1
                 FOR UPDATE",
            )
            .bind(next_input.input_batch_id.trim())
            .fetch_optional(&mut **tx)
            .await
        }
    }
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some((candidate_order_id, candidate_status)) = candidate else {
        return Err(ProductionMapError::MergeInputNotAccepted);
    };
    if candidate_order_id.trim() != event.order_id.trim() {
        return Err(ProductionMapError::MergeInputNotAccepted);
    }
    if candidate_status.trim() != "waiting" {
        return Err(ProductionMapError::MergeInputAlreadyUsed);
    }
    Ok(())
}

fn merge_session_transition_matches(
    current_payload: &serde_json::Value,
    proposed_session: &OrderRunSession,
) -> bool {
    let Ok(current_links) = order_run_input_links_from_payload(current_payload) else {
        return false;
    };
    let current_active = current_links
        .iter()
        .find(|link| link.status == OrderRunInputStatus::InUse)
        .map(|link| link.input_batch_id.trim())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            current_links.is_empty().then(|| {
                current_payload
                    .get("input_progress_batch_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .trim()
            })
        })
        .unwrap_or_default();
    let Ok(proposed_links) =
        order_run_input_links_from_payload(&proposed_session.payload_json)
    else {
        return false;
    };
    let proposed_active = proposed_links
        .iter()
        .find(|link| link.status == OrderRunInputStatus::InUse)
        .map(|link| link.input_batch_id.trim())
        .unwrap_or_default();
    let merge_from = proposed_session
        .payload_json
        .get("merge_from_input_batch_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim();
    let merge_to = proposed_session
        .payload_json
        .get("merge_to_input_batch_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim();

    !current_active.is_empty()
        && current_active == merge_from
        && !merge_to.is_empty()
        && merge_to == proposed_active
        && proposed_links.iter().any(|link| {
            link.input_batch_id.trim() == merge_from
                && link.status == OrderRunInputStatus::Processed
        })
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

struct StoredQueueEventIdentity<'a> {
    apparatus: Option<&'a str>,
    order_id: &'a str,
    action: &'a str,
    from_state: &'a str,
    to_state: &'a str,
    policy: &'a str,
    actor_role: &'a str,
    actor_ref: &'a str,
    actor_display_name: &'a str,
    assigned_apparatus: &'a serde_json::Value,
    payload: &'a serde_json::Value,
}

fn queue_event_identity_matches(
    existing: &StoredQueueEventIdentity<'_>,
    apparatus_id: &ApparatusId,
    event: &ApparatusQueueActionEvent,
    assigned_apparatus: &[String],
) -> Result<bool, ProductionMapError> {
    let assigned_apparatus =
        serde_json::to_value(assigned_apparatus).map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(existing.apparatus == Some(apparatus_id.as_str())
        && existing.order_id.trim() == event.order_id.trim()
        && existing.action == queue_action_as_str(event.action)
        && existing.from_state == event.from_state.as_str()
        && existing.to_state == event.to_state.as_str()
        && existing.policy == event.policy.as_str()
        && existing.actor_role.trim() == event.actor.role.trim()
        && existing.actor_ref.trim() == event.actor.ref_.trim()
        && existing.actor_display_name.trim() == event.actor.display_name.trim()
        && existing.assigned_apparatus == &assigned_apparatus
        && existing.payload == &event.payload_json)
}

pub(super) fn queue_action_from_str(
    value: &str,
) -> Option<crate::core::production_map::queue_state::ApparatusQueueAction> {
    use crate::core::production_map::queue_state::ApparatusQueueAction;

    let value = value.trim();
    if value.eq_ignore_ascii_case("start") {
        Some(ApparatusQueueAction::Start)
    } else if value.eq_ignore_ascii_case("pause") {
        Some(ApparatusQueueAction::Pause)
    } else if value.eq_ignore_ascii_case("freeze") {
        Some(ApparatusQueueAction::Freeze)
    } else if value.eq_ignore_ascii_case("detach_roll") {
        Some(ApparatusQueueAction::DetachRoll)
    } else if value.eq_ignore_ascii_case("resume") {
        Some(ApparatusQueueAction::Resume)
    } else if value.eq_ignore_ascii_case("merge") {
        Some(ApparatusQueueAction::Merge)
    } else if value.eq_ignore_ascii_case("roll_complete") {
        Some(ApparatusQueueAction::RollComplete)
    } else if value.eq_ignore_ascii_case("complete") {
        Some(ApparatusQueueAction::Complete)
    } else {
        None
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
        crate::core::production_map::queue_state::ApparatusQueueAction::Merge => "merge",
        crate::core::production_map::queue_state::ApparatusQueueAction::RollComplete => {
            "roll_complete"
        }
        crate::core::production_map::queue_state::ApparatusQueueAction::Complete => "complete",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        StoredQueueEventIdentity, merge_session_transition_matches, queue_event_identity_matches,
    };
    use crate::core::apparatus_standard::ApparatusId;
    use crate::core::production_map::{
        ApparatusQueueActionEvent, ApparatusQueuePolicy, OrderRunSession, OrderRunStatus,
        QueueActionActor,
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
        let existing_assigned = serde_json::json!(["apparatus:test:a"]);
        let existing = StoredQueueEventIdentity {
            apparatus: Some("apparatus:test:a"),
            order_id: "zakaz-1",
            action: "start",
            from_state: "pending",
            to_state: "in_progress",
            policy: "free_pick",
            actor_role: "operator",
            actor_ref: "worker-1",
            actor_display_name: "Worker",
            assigned_apparatus: &existing_assigned,
            payload: &event.payload_json,
        };
        let matching =
            queue_event_identity_matches(&existing, &apparatus_id, &event, &assigned).unwrap();
        assert!(matching);

        let changed_payload = serde_json::json!({"request": "two"});
        let changed = StoredQueueEventIdentity {
            payload: &changed_payload,
            ..existing
        };
        let changed =
            queue_event_identity_matches(&changed, &apparatus_id, &event, &assigned).unwrap();
        assert!(!changed);
    }

    #[test]
    fn merge_write_rejects_a_stale_current_input() {
        let proposed = OrderRunSession {
            session_id: "session-1".to_string(),
            apparatus: "apparatus:test:rezka".to_string(),
            order_id: "zakaz-1".to_string(),
            status: OrderRunStatus::Active,
            worker_role: "aparatchi".to_string(),
            worker_ref: "worker-1".to_string(),
            worker_display_name: "Worker".to_string(),
            started_at_unix: 1,
            updated_at_unix: 2,
            payload_json: serde_json::json!({
                "merge_from_input_batch_id": "wip-a",
                "merge_to_input_batch_id": "wip-b",
                "input_progress_batch_id": "wip-b",
                "input_lineage": [
                    {
                        "input_batch_id": "wip-a",
                        "input_qr_payload": "qr-a",
                        "source_apparatus": "apparatus:test:lamination",
                        "source_kind": "progress_batch",
                        "stage_node_id": "rezka",
                        "sequence_no": 1,
                        "status": "processed",
                        "linked_at_unix": 1,
                        "processed_at_unix": 2
                    },
                    {
                        "input_batch_id": "wip-b",
                        "input_qr_payload": "qr-b",
                        "source_apparatus": "apparatus:test:lamination",
                        "source_kind": "progress_batch",
                        "stage_node_id": "rezka",
                        "sequence_no": 2,
                        "status": "in_use",
                        "linked_at_unix": 2
                    }
                ]
            }),
        };

        assert!(merge_session_transition_matches(
            &serde_json::json!({"input_progress_batch_id": "wip-a"}),
            &proposed,
        ));
        assert!(!merge_session_transition_matches(
            &serde_json::json!({"input_progress_batch_id": "wip-b"}),
            &proposed,
        ));
    }
}
