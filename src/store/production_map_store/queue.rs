use std::collections::BTreeMap;

use rusqlite::params;

use super::{ProductionMapStore, unix_micros};
use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    ApparatusQueueActionEvent, ProductionMapError, ProductionMapStorePort,
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
        let apparatus = ApparatusId::new(apparatus).map_err(|_| ProductionMapError::StoreFailed)?;
        grouped
            .entry(apparatus.to_string())
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
    let apparatus = ApparatusId::new(apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let conn = store
        .conn
        .lock()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    conn.execute(
        "DELETE FROM apparatus_queue_states WHERE apparatus = ?1",
        params![apparatus.as_str()],
    )
    .map_err(|_| ProductionMapError::StoreFailed)?;
    for (order_id, state) in states {
        conn.execute(
            "INSERT INTO apparatus_queue_states (apparatus, order_id, state, saved_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                apparatus.as_str(),
                order_id.trim(),
                state.trim(),
                unix_micros().to_string()
            ],
        )
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
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
    let apparatus = ApparatusId::new(event.apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::StoreFailed)?;
    for assigned_apparatus in &event.assigned_apparatus {
        ApparatusId::new(assigned_apparatus.trim().to_string())
            .map_err(|_| ProductionMapError::StoreFailed)?;
    }
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
            apparatus.as_str(),
            event.order_id.trim(),
            match event.action {
                crate::core::production_map::queue_state::ApparatusQueueAction::Start => "start",
                crate::core::production_map::queue_state::ApparatusQueueAction::Pause => "pause",
                crate::core::production_map::queue_state::ApparatusQueueAction::Freeze => "freeze",
                crate::core::production_map::queue_state::ApparatusQueueAction::DetachRoll =>
                    "detach_roll",
                crate::core::production_map::queue_state::ApparatusQueueAction::Resume => "resume",
                crate::core::production_map::queue_state::ApparatusQueueAction::RollComplete =>
                    "roll_complete",
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
