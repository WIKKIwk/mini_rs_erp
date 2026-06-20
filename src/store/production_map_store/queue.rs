use std::collections::BTreeMap;

use rusqlite::params;

use super::{ProductionMapStore, unix_micros};
use crate::core::production_map::{
    ApparatusQueueActionEvent, ApparatusQueuePolicy, ProductionMapError, ProductionMapStorePort,
    QueueActionActor,
};

pub(super) async fn apparatus_queue_states(
    store: &ProductionMapStore,
) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut stmt = conn
        .prepare("SELECT apparatus, order_id, state FROM apparatus_queue_states")
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut grouped = BTreeMap::<String, BTreeMap<String, String>>::new();
    for row in rows {
        let (apparatus, order_id, state) = row.map_err(|_| ProductionMapError::StoreFailed)?;
        grouped
            .entry(apparatus)
            .or_default()
            .insert(order_id, state);
    }
    Ok(grouped)
}

pub(super) async fn put_apparatus_queue_states(
    store: &ProductionMapStore,
    apparatus: &str,
    states: BTreeMap<String, String>,
) -> Result<(), ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let apparatus = apparatus.trim();
    conn.execute(
        "DELETE FROM apparatus_queue_states WHERE apparatus = ?1",
        params![apparatus],
    )
    .map_err(|_| ProductionMapError::StoreFailed)?;
    for (order_id, state) in states {
        conn.execute(
            "INSERT INTO apparatus_queue_states (apparatus, order_id, state, saved_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                apparatus,
                order_id.trim(),
                state.trim(),
                unix_micros().to_string()
            ],
        )
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}

pub(super) async fn apparatus_queue_policies(
    store: &ProductionMapStore,
) -> Result<BTreeMap<String, ApparatusQueuePolicy>, ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut stmt = conn
        .prepare("SELECT apparatus, policy FROM apparatus_queue_policies")
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut result = BTreeMap::new();
    for row in rows {
        let (apparatus, policy) = row.map_err(|_| ProductionMapError::StoreFailed)?;
        let policy = ApparatusQueuePolicy::parse(&policy).ok_or(ProductionMapError::StoreFailed)?;
        result.insert(apparatus, policy);
    }
    Ok(result)
}

pub(super) async fn put_apparatus_queue_policy(
    store: &ProductionMapStore,
    apparatus: &str,
    policy: ApparatusQueuePolicy,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let payload = serde_json::json!({
        "actor": actor,
        "policy": policy.as_str(),
    });
    conn.execute(
        "INSERT INTO apparatus_queue_policies
            (apparatus, policy, actor_role, actor_ref, actor_display_name, payload_json, saved_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(apparatus) DO UPDATE SET
            policy = excluded.policy,
            actor_role = excluded.actor_role,
            actor_ref = excluded.actor_ref,
            actor_display_name = excluded.actor_display_name,
            payload_json = excluded.payload_json,
            saved_at = excluded.saved_at",
        params![
            apparatus.trim(),
            policy.as_str(),
            actor.role.trim(),
            actor.ref_.trim(),
            actor.display_name.trim(),
            payload.to_string(),
            unix_micros().to_string(),
        ],
    )
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn put_apparatus_queue_states_with_event(
    store: &ProductionMapStore,
    apparatus: &str,
    states: BTreeMap<String, String>,
    event: ApparatusQueueActionEvent,
) -> Result<(), ProductionMapError> {
    store.put_apparatus_queue_states(apparatus, states).await?;
    store.append_apparatus_queue_action_event(event).await
}

pub(super) async fn append_apparatus_queue_action_event(
    store: &ProductionMapStore,
    event: ApparatusQueueActionEvent,
) -> Result<(), ProductionMapError> {
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    conn.execute(
        "INSERT INTO apparatus_queue_action_events
            (event_id, apparatus, order_id, action, from_state, to_state, policy,
             actor_role, actor_ref, actor_display_name, assigned_apparatus_json,
             payload_json, saved_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            event.event_id.trim(),
            event.apparatus.trim(),
            event.order_id.trim(),
            match event.action {
                crate::core::production_map::queue_state::ApparatusQueueAction::Start => "start",
                crate::core::production_map::queue_state::ApparatusQueueAction::Pause => "pause",
                crate::core::production_map::queue_state::ApparatusQueueAction::Resume => "resume",
                crate::core::production_map::queue_state::ApparatusQueueAction::Complete =>
                    "complete",
            },
            event.from_state.as_str(),
            event.to_state.as_str(),
            event.policy.as_str(),
            event.actor.role.trim(),
            event.actor.ref_.trim(),
            event.actor.display_name.trim(),
            serde_json::to_string(&event.assigned_apparatus)
                .map_err(|_| ProductionMapError::StoreFailed)?,
            event.payload_json.to_string(),
            unix_micros().to_string(),
        ],
    )
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}
