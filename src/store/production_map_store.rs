use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::{Connection, params};

use crate::core::production_map::{
    ApparatusMaterialRule, ApparatusQueueActionEvent, ApparatusQueuePolicy,
    ProductionMapDefinition, ProductionMapError, ProductionMapStorePort, QueueActionActor,
    RawMaterialAssignment,
};

mod map_helpers;
mod migration;
#[cfg(test)]
mod tests;

use self::map_helpers::{
    put_map_inner, reject_duplicate_order_number, reject_order_number_immutable,
};
use self::migration::{configure_connection, migrate};

#[derive(Clone)]
pub struct ProductionMapStore {
    conn: Arc<Mutex<Connection>>,
}

impl ProductionMapStore {
    pub fn new(path: PathBuf) -> Self {
        Self::open(path).unwrap_or_else(|error| {
            panic!("production map sqlite store unavailable: {error}");
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, ProductionMapError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| ProductionMapError::StoreFailed)?;
        }
        let conn = Connection::open(path).map_err(|_| ProductionMapError::StoreFailed)?;
        configure_connection(&conn).map_err(|_| ProductionMapError::StoreFailed)?;
        migrate(&conn).map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

#[async_trait]
impl ProductionMapStorePort for ProductionMapStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json
                 FROM production_maps
                 ORDER BY saved_at DESC",
            )
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let rows = stmt
            .query_map([], |row| {
                let payload: String = row.get(0)?;
                let map = serde_json::from_str::<ProductionMapDefinition>(&payload)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
                Ok(map)
            })
            .map_err(|_| ProductionMapError::StoreFailed)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        reject_order_number_immutable(&conn, &map)?;
        reject_duplicate_order_number(&conn, &map)?;
        put_map_inner(&conn, &map)
    }

    async fn put_maps_batch(
        &self,
        maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        conn.execute("BEGIN IMMEDIATE", [])
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let result = (|| {
            for map in maps {
                reject_order_number_immutable(&conn, map)?;
                reject_duplicate_order_number(&conn, map)?;
                put_map_inner(&conn, map)?;
            }
            Ok::<(), ProductionMapError>(())
        })();
        if result.is_ok() {
            conn.execute("COMMIT", [])
                .map_err(|_| ProductionMapError::StoreFailed)?;
        } else {
            let _ = conn.execute("ROLLBACK", []);
        }
        result
    }

    async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        conn.execute(
            "DELETE FROM production_maps WHERE id = ?1",
            params![map_id.trim()],
        )
        .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(())
    }

    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut stmt = conn
            .prepare("SELECT apparatus, order_ids_json FROM apparatus_sequences")
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let rows = stmt
            .query_map([], |row| {
                let apparatus: String = row.get(0)?;
                let payload: String = row.get(1)?;
                let order_ids = serde_json::from_str::<Vec<String>>(&payload)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
                Ok((apparatus, order_ids))
            })
            .map_err(|_| ProductionMapError::StoreFailed)?;
        rows.collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let payload =
            serde_json::to_string(&order_ids).map_err(|_| ProductionMapError::StoreFailed)?;
        conn.execute(
            "INSERT INTO apparatus_sequences (apparatus, order_ids_json, saved_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(apparatus) DO UPDATE SET
                order_ids_json = excluded.order_ids_json,
                saved_at = excluded.saved_at",
            params![apparatus.trim(), payload, unix_micros().to_string()],
        )
        .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(())
    }

    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        let conn = self
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

    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        let conn = self
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

    async fn apparatus_queue_policies(
        &self,
    ) -> Result<BTreeMap<String, ApparatusQueuePolicy>, ProductionMapError> {
        let conn = self
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
            let policy =
                ApparatusQueuePolicy::parse(&policy).ok_or(ProductionMapError::StoreFailed)?;
            result.insert(apparatus, policy);
        }
        Ok(result)
    }

    async fn put_apparatus_queue_policy(
        &self,
        apparatus: &str,
        policy: ApparatusQueuePolicy,
        actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        let conn = self
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

    async fn put_apparatus_queue_states_with_event(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        self.put_apparatus_queue_states(apparatus, states).await?;
        self.append_apparatus_queue_action_event(event).await
    }

    async fn append_apparatus_queue_action_event(
        &self,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        let conn = self
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
                    crate::core::production_map::queue_state::ApparatusQueueAction::Start =>
                        "start",
                    crate::core::production_map::queue_state::ApparatusQueueAction::Pause =>
                        "pause",
                    crate::core::production_map::queue_state::ApparatusQueueAction::Resume =>
                        "resume",
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

    async fn apparatus_material_rules(
        &self,
    ) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_material_rule(
        &self,
        _rule: ApparatusMaterialRule,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }

    async fn put_raw_material_assignment(
        &self,
        _assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        Err(ProductionMapError::StoreFailed)
    }
}

fn unix_micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}
