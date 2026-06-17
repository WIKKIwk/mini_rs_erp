use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};

use crate::core::production_map::{
    ApparatusMaterialRule, ApparatusQueueActionEvent, ApparatusQueuePolicy,
    ProductionMapDefinition, ProductionMapError, ProductionMapStorePort, QueueActionActor,
    RawMaterialAssignment,
};

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
        let conn = self
            .conn
            .lock()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut stmt = conn
            .prepare("SELECT payload_json FROM apparatus_material_rules ORDER BY lower(apparatus)")
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let rows = stmt
            .query_map([], |row| {
                let payload: String = row.get(0)?;
                let rule = serde_json::from_str::<ApparatusMaterialRule>(&payload)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
                Ok(rule)
            })
            .map_err(|_| ProductionMapError::StoreFailed)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn put_apparatus_material_rule(
        &self,
        rule: ApparatusMaterialRule,
    ) -> Result<(), ProductionMapError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let item_groups = serde_json::to_string(&rule.item_groups)
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let payload = serde_json::to_string(&rule).map_err(|_| ProductionMapError::StoreFailed)?;
        conn.execute(
            "INSERT INTO apparatus_material_rules
                (apparatus, item_groups_json, payload_json, saved_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(apparatus) DO UPDATE SET
                item_groups_json = excluded.item_groups_json,
                payload_json = excluded.payload_json,
                saved_at = excluded.saved_at",
            params![
                rule.apparatus.trim(),
                item_groups,
                payload,
                unix_micros().to_string(),
            ],
        )
        .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(())
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut stmt = conn
            .prepare("SELECT payload_json FROM raw_material_assignments ORDER BY saved_at DESC")
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let rows = stmt
            .query_map([], |row| {
                let payload: String = row.get(0)?;
                let assignment = serde_json::from_str::<RawMaterialAssignment>(&payload)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
                Ok(assignment)
            })
            .map_err(|_| ProductionMapError::StoreFailed)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let payload =
            serde_json::to_string(&assignment).map_err(|_| ProductionMapError::StoreFailed)?;
        conn.execute(
            "INSERT INTO raw_material_assignments
                (barcode, order_id, apparatus, item_code, item_group, payload_json, saved_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                assignment.barcode.trim(),
                assignment.order_id.trim(),
                assignment.apparatus.trim(),
                assignment.item_code.trim(),
                assignment.item_group.trim(),
                payload,
                unix_micros().to_string(),
            ],
        )
        .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(())
    }
}

fn put_map_inner(
    conn: &Connection,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let payload = serde_json::to_string(map).map_err(|_| ProductionMapError::StoreFailed)?;
    conn.execute(
        "INSERT INTO production_maps
            (id, product_code, title, saved_at, payload_json)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
            product_code = excluded.product_code,
            title = excluded.title,
            saved_at = excluded.saved_at,
            payload_json = excluded.payload_json",
        params![
            map.id.trim(),
            map.product_code.trim(),
            map.title.trim(),
            unix_micros().to_string(),
            payload
        ],
    )
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

fn reject_order_number_immutable(
    conn: &Connection,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let id = map.id.trim();
    if !id.starts_with("zakaz-") {
        return Ok(());
    }
    let order_number = map.order_number.trim();
    if order_number.is_empty() {
        return Ok(());
    }
    let existing = conn
        .query_row(
            "SELECT payload_json FROM production_maps WHERE id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some(payload) = existing else {
        return Ok(());
    };
    let existing_map = serde_json::from_str::<ProductionMapDefinition>(&payload)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let existing_number = existing_map.order_number.trim();
    if !existing_number.is_empty() && existing_number != order_number {
        return Err(ProductionMapError::OrderNumberImmutable);
    }
    Ok(())
}

fn reject_duplicate_order_number(
    conn: &Connection,
    map: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let order_number = map.order_number.trim();
    if order_number.is_empty() {
        return Ok(());
    }
    let mut stmt = conn
        .prepare("SELECT payload_json FROM production_maps")
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| ProductionMapError::StoreFailed)?;
    for row in rows {
        let payload = row.map_err(|_| ProductionMapError::StoreFailed)?;
        let existing = serde_json::from_str::<ProductionMapDefinition>(&payload)
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if existing.order_number.trim() == order_number && !is_same_zakaz(&existing, map) {
            return Err(ProductionMapError::DuplicateOrderNumber);
        }
    }
    Ok(())
}

fn is_same_zakaz(existing: &ProductionMapDefinition, next: &ProductionMapDefinition) -> bool {
    existing.id.trim() == next.id.trim()
        && existing.title.trim() == next.title.trim()
        && existing.product_code.trim() == next.product_code.trim()
}

fn configure_connection(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS production_maps (
            id TEXT PRIMARY KEY,
            product_code TEXT NOT NULL,
            title TEXT NOT NULL,
            saved_at TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_production_maps_saved
            ON production_maps(saved_at DESC);
        CREATE INDEX IF NOT EXISTS idx_production_maps_product_code
            ON production_maps(product_code);
        CREATE TABLE IF NOT EXISTS apparatus_sequences (
            apparatus TEXT PRIMARY KEY,
            order_ids_json TEXT NOT NULL,
            saved_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS apparatus_queue_states (
            apparatus TEXT NOT NULL,
            order_id TEXT NOT NULL,
            state TEXT NOT NULL,
            saved_at TEXT NOT NULL,
            PRIMARY KEY (apparatus, order_id)
        );
        CREATE TABLE IF NOT EXISTS apparatus_queue_policies (
            apparatus TEXT PRIMARY KEY,
            policy TEXT NOT NULL,
            actor_role TEXT NOT NULL DEFAULT '',
            actor_ref TEXT NOT NULL DEFAULT '',
            actor_display_name TEXT NOT NULL DEFAULT '',
            payload_json TEXT NOT NULL DEFAULT '{}',
            saved_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS apparatus_queue_action_events (
            event_id TEXT PRIMARY KEY,
            apparatus TEXT NOT NULL,
            order_id TEXT NOT NULL,
            action TEXT NOT NULL,
            from_state TEXT NOT NULL,
            to_state TEXT NOT NULL,
            policy TEXT NOT NULL,
            actor_role TEXT NOT NULL DEFAULT '',
            actor_ref TEXT NOT NULL DEFAULT '',
            actor_display_name TEXT NOT NULL DEFAULT '',
            assigned_apparatus_json TEXT NOT NULL DEFAULT '[]',
            payload_json TEXT NOT NULL DEFAULT '{}',
            saved_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS apparatus_material_rules (
            apparatus TEXT PRIMARY KEY,
            item_groups_json TEXT NOT NULL,
            payload_json TEXT NOT NULL DEFAULT '{}',
            saved_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS raw_material_assignments (
            barcode TEXT PRIMARY KEY,
            order_id TEXT NOT NULL,
            apparatus TEXT NOT NULL,
            item_code TEXT NOT NULL,
            item_group TEXT NOT NULL,
            payload_json TEXT NOT NULL DEFAULT '{}',
            saved_at TEXT NOT NULL
        );",
    )
}

fn unix_micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::production_map::{
        ProductionMapNode, ProductionMapNodeKind, ProductionMapService,
    };

    #[tokio::test]
    async fn production_map_store_persists_maps_in_sqlite() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mobile_production_maps.sqlite");
        let service = ProductionMapService::new(Arc::new(ProductionMapStore::new(path.clone())));

        service
            .upsert_map(ProductionMapDefinition {
                id: "map-1".to_string(),
                product_code: "HOT".to_string(),
                title: "Hot".to_string(),
                code: "Z-HOT-1".to_string(),
                order_number: "1234".to_string(),
                roll_count: Some(7.0),
                width_mm: Some(650.0),
                nodes: vec![
                    ProductionMapNode {
                        id: "start".to_string(),
                        kind: ProductionMapNodeKind::Start,
                        title: "Start".to_string(),
                        formula: None,
                        role_code: String::new(),
                        item_code: String::new(),
                        qty_formula: String::new(),
                        from_location: String::new(),
                        to_location: String::new(),
                        alternative_group_id: String::new(),
                        alternative_group_label: String::new(),
                        alternative_assigned_title: String::new(),
                        x: 0.0,
                        y: 0.0,
                    },
                    ProductionMapNode {
                        id: "apparatus".to_string(),
                        kind: ProductionMapNodeKind::Apparatus,
                        title: "Extrujen aparat - A".to_string(),
                        formula: None,
                        role_code: String::new(),
                        item_code: String::new(),
                        qty_formula: String::new(),
                        from_location: String::new(),
                        to_location: String::new(),
                        alternative_group_id: String::new(),
                        alternative_group_label: String::new(),
                        alternative_assigned_title: String::new(),
                        x: 0.0,
                        y: 132.0,
                    },
                    ProductionMapNode {
                        id: "end".to_string(),
                        kind: ProductionMapNodeKind::End,
                        title: "End".to_string(),
                        formula: None,
                        role_code: String::new(),
                        item_code: String::new(),
                        qty_formula: String::new(),
                        from_location: String::new(),
                        to_location: String::new(),
                        alternative_group_id: String::new(),
                        alternative_group_label: String::new(),
                        alternative_assigned_title: String::new(),
                        x: 0.0,
                        y: 264.0,
                    },
                ],
                edges: vec![
                    crate::core::production_map::ProductionMapEdge {
                        from: "start".to_string(),
                        to: "apparatus".to_string(),
                        branch: String::new(),
                    },
                    crate::core::production_map::ProductionMapEdge {
                        from: "apparatus".to_string(),
                        to: "end".to_string(),
                        branch: String::new(),
                    },
                ],
            })
            .await
            .expect("save map");
        drop(service);

        let conn = rusqlite::Connection::open(&path).expect("open sqlite");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM production_maps", [], |row| row.get(0))
            .expect("count maps");
        assert_eq!(count, 1);
        drop(conn);

        let reloaded = ProductionMapService::new(Arc::new(ProductionMapStore::new(path)));
        let maps = reloaded.maps().await.expect("maps");
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].map.product_code, "HOT");
        assert_eq!(maps[0].map.order_number, "1234");
        assert_eq!(maps[0].map.roll_count, Some(7.0));
        assert_eq!(maps[0].map.width_mm, Some(650.0));
        assert_eq!(maps[0].program.operations.len(), 3);
        assert_eq!(maps[0].program.operations[1].op_code, "apparatus");

        let duplicate = reloaded
            .upsert_map(ProductionMapDefinition {
                id: "map-2".to_string(),
                product_code: "OTHER".to_string(),
                title: "Other".to_string(),
                code: String::new(),
                order_number: "1234".to_string(),
                roll_count: None,
                width_mm: None,
                nodes: maps[0].map.nodes.clone(),
                edges: maps[0].map.edges.clone(),
            })
            .await;
        assert_eq!(duplicate, Err(ProductionMapError::DuplicateOrderNumber));
    }
}
