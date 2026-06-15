use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};

use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapError, ProductionMapStorePort,
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

    async fn daily_work_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let mut stmt = conn
            .prepare("SELECT work_date, order_ids_json FROM daily_work_sequences")
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let rows = stmt
            .query_map([], |row| {
                let work_date: String = row.get(0)?;
                let payload: String = row.get(1)?;
                let order_ids = serde_json::from_str::<Vec<String>>(&payload)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
                Ok((work_date, order_ids))
            })
            .map_err(|_| ProductionMapError::StoreFailed)?;
        rows.collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(|_| ProductionMapError::StoreFailed)
    }

    async fn put_daily_work_sequence(
        &self,
        work_date: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let order_ids = order_ids
            .into_iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        if order_ids.is_empty() {
            conn.execute(
                "DELETE FROM daily_work_sequences WHERE work_date = ?1",
                params![work_date.trim()],
            )
            .map_err(|_| ProductionMapError::StoreFailed)?;
            return Ok(());
        }
        let payload =
            serde_json::to_string(&order_ids).map_err(|_| ProductionMapError::StoreFailed)?;
        conn.execute(
            "INSERT INTO daily_work_sequences (work_date, order_ids_json, saved_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(work_date) DO UPDATE SET
                order_ids_json = excluded.order_ids_json,
                saved_at = excluded.saved_at",
            params![work_date.trim(), payload, unix_micros().to_string()],
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
        CREATE TABLE IF NOT EXISTS daily_work_sequences (
            work_date TEXT PRIMARY KEY,
            order_ids_json TEXT NOT NULL,
            saved_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS apparatus_queue_states (
            apparatus TEXT NOT NULL,
            order_id TEXT NOT NULL,
            state TEXT NOT NULL,
            saved_at TEXT NOT NULL,
            PRIMARY KEY (apparatus, order_id)
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
