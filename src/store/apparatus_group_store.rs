use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};

use crate::core::apparatus_groups::{
    ApparatusCatalogEntry, ApparatusGroup, ApparatusGroupError, ApparatusGroupStorePort,
    ApparatusMasterData, ApparatusSource, apparatus_master_data_for_name, custom_apparatus_id,
};

#[derive(Clone)]
pub struct ApparatusGroupStore {
    conn: Arc<Mutex<Connection>>,
}

impl ApparatusGroupStore {
    pub fn new(path: PathBuf) -> Self {
        Self::open(path).unwrap_or_else(|error| {
            panic!("apparatus group sqlite store unavailable: {error}");
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, ApparatusGroupError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| ApparatusGroupError::StoreFailed)?;
        }
        let conn = Connection::open(path).map_err(|_| ApparatusGroupError::StoreFailed)?;
        configure_connection(&conn).map_err(|_| ApparatusGroupError::StoreFailed)?;
        migrate(&conn).map_err(|_| ApparatusGroupError::StoreFailed)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

#[async_trait]
impl ApparatusGroupStorePort for ApparatusGroupStore {
    async fn groups(&self) -> Result<Vec<ApparatusGroup>, ApparatusGroupError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json
                 FROM apparatus_groups
                 ORDER BY lower_name ASC",
            )
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let rows = stmt
            .query_map([], |row| {
                let payload: String = row.get(0)?;
                let group = serde_json::from_str::<ApparatusGroup>(&payload)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
                Ok(group)
            })
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| ApparatusGroupError::StoreFailed)
    }

    async fn put_group(&self, group: ApparatusGroup) -> Result<(), ApparatusGroupError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let payload =
            serde_json::to_string(&group).map_err(|_| ApparatusGroupError::StoreFailed)?;
        conn.execute(
            "INSERT INTO apparatus_groups (name, lower_name, payload_json, saved_at)
             VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(lower_name) DO UPDATE SET
               name = excluded.name,
               payload_json = excluded.payload_json,
               saved_at = excluded.saved_at",
            params![group.name, group.name.to_lowercase(), payload],
        )
        .map_err(|_| ApparatusGroupError::StoreFailed)?;
        Ok(())
    }

    async fn apparatus(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>, ApparatusGroupError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let needle = query.trim().to_lowercase();
        let pattern = format!("%{needle}%");
        let mut stmt = conn
            .prepare(
                "SELECT name
                 FROM apparatus
                 WHERE (?1 = '' OR lower_name LIKE ?2)
                 ORDER BY lower_name ASC
                 LIMIT ?3",
            )
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let rows = stmt
            .query_map(params![needle, pattern, limit.max(1) as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| ApparatusGroupError::StoreFailed)
    }

    async fn apparatus_catalog(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ApparatusCatalogEntry>, ApparatusGroupError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let needle = query.trim().to_lowercase();
        let pattern = format!("%{needle}%");
        let mut stmt = conn
            .prepare(
                "SELECT id, name, payload_json
                 FROM apparatus
                 WHERE (?1 = '' OR lower_name LIKE ?2)
                 ORDER BY lower_name ASC
                 LIMIT ?3",
            )
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let rows = stmt
            .query_map(params![needle, pattern, limit.max(1) as i64], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let payload: String = row.get(2)?;
                let master = serde_json::from_str::<ApparatusMasterData>(&payload)
                    .unwrap_or_else(|_| apparatus_master_data_for_name(&name));
                Ok(ApparatusCatalogEntry {
                    id,
                    name,
                    source: ApparatusSource::Custom,
                    sort_order: 0,
                    master,
                })
            })
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| ApparatusGroupError::StoreFailed)
    }

    async fn put_apparatus(&self, name: &str) -> Result<String, ApparatusGroupError> {
        self.save_apparatus(None, name, &apparatus_master_data_for_name(name))
            .await
            .map(|_| name.trim().to_string())
    }

    async fn put_apparatus_with_master_data(
        &self,
        name: &str,
        master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
        self.save_apparatus(None, name, master)
            .await
            .map(|_| name.trim().to_string())
    }

    async fn put_apparatus_with_id(
        &self,
        requested_id: Option<&str>,
        name: &str,
        master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
        self.save_apparatus(requested_id, name, master).await
    }
}

impl ApparatusGroupStore {
    async fn save_apparatus(
        &self,
        requested_id: Option<&str>,
        name: &str,
        master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        let lower_name = name.to_lowercase();
        let mut payload =
            serde_json::to_value(master).map_err(|_| ApparatusGroupError::StoreFailed)?;
        payload["warehouse"] = serde_json::Value::String(name.to_string());
        let conn = self
            .conn
            .lock()
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let existing_id = if let Some(requested_id) = requested_id {
            conn.query_row(
                "SELECT id FROM apparatus WHERE id = ?1",
                params![requested_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
        } else {
            conn.query_row(
                "SELECT id FROM apparatus WHERE lower_name = ?1",
                params![lower_name],
                |row| row.get::<_, String>(0),
            )
            .optional()
        }
        .map_err(|_| ApparatusGroupError::StoreFailed)?;
        if let Some(existing_id) = existing_id {
            conn.execute(
                "UPDATE apparatus
                 SET lower_name = ?1, name = ?2, payload_json = ?3,
                     saved_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?4",
                params![lower_name, name, payload.to_string(), existing_id],
            )
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
            return Ok(existing_id);
        }
        let id = requested_id
            .filter(|id| !id.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| custom_apparatus_id(name));
        conn.execute(
            "INSERT INTO apparatus (id, lower_name, name, payload_json, saved_at)
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, lower_name, name, payload.to_string()],
        )
        .map_err(|_| ApparatusGroupError::StoreFailed)?;
        Ok(id)
    }
}

fn configure_connection(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS apparatus_groups (
            lower_name TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            saved_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_apparatus_groups_name
            ON apparatus_groups(lower_name);
        CREATE TABLE IF NOT EXISTS apparatus (
            id TEXT NOT NULL,
            lower_name TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            payload_json TEXT NOT NULL DEFAULT '{}',
            saved_at TEXT NOT NULL
        );
        ",
    )?;
    if !table_has_column(conn, "payload_json")? {
        conn.execute(
            "ALTER TABLE apparatus ADD COLUMN payload_json TEXT NOT NULL DEFAULT '{}'",
            [],
        )?;
    }
    if !table_has_column(conn, "id")? {
        conn.execute("ALTER TABLE apparatus ADD COLUMN id TEXT", [])?;
    }
    conn.execute(
        "UPDATE apparatus
         SET id = 'apparatus:' || lower_name
         WHERE id IS NULL OR trim(id) = ''",
        [],
    )?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_apparatus_name
            ON apparatus(lower_name);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_apparatus_id
            ON apparatus(id);",
    )
}

fn table_has_column(conn: &Connection, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare("PRAGMA table_info(apparatus)")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::apparatus_groups::{
        ApparatusGroupService, ApparatusGroupUpsert, ApparatusMasterData,
    };

    #[tokio::test]
    async fn apparatus_group_store_persists_groups_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("apparatus_groups.sqlite");
        let service = ApparatusGroupService::new(Arc::new(
            ApparatusGroupStore::open(&path).expect("open apparatus group store"),
        ));

        let saved = service
            .upsert_group(ApparatusGroupUpsert {
                name: " pechat ".to_string(),
                apparatus: vec![
                    "7 ta rangli pechat".to_string(),
                    "8 ta rangli pechat".to_string(),
                    "7 TA RANGLI PECHAT".to_string(),
                ],
            })
            .await
            .expect("save apparatus group");
        assert_eq!(saved.name, "Bosma aparat");
        assert_eq!(
            saved.apparatus,
            vec![
                "7 ta rangli bosma aparat".to_string(),
                "8 ta rangli bosma aparat".to_string(),
                "9 ta rangli bosma aparat".to_string(),
                "Flexo pechat".to_string(),
            ]
        );

        let reloaded = ApparatusGroupStore::open(&path).expect("reopen apparatus group store");
        let groups = reloaded.groups().await.expect("load apparatus groups");

        assert_eq!(groups, vec![saved]);

        let created = reloaded
            .put_apparatus(" Bobst 1 ")
            .await
            .expect("save apparatus");
        assert_eq!(created, "Bobst 1");
        assert_eq!(
            reloaded.apparatus("bob", 20).await.expect("list apparatus"),
            vec!["Bobst 1".to_string()]
        );
        let catalog = ApparatusGroupService::new(Arc::new(reloaded.clone()))
            .apparatus_catalog("bob", 20)
            .await
            .expect("load apparatus metadata");
        assert_eq!(catalog[0].master.family, "other");
        assert_eq!(catalog[0].master.kind, "other");
        assert_eq!(catalog[0].master.capabilities, vec!["apparatus"]);

        let stable_id = reloaded
            .put_apparatus_with_id(
                Some("apparatus:bobst 1"),
                "Bobst 2",
                &ApparatusMasterData {
                    family: "pechat".to_string(),
                    kind: "flexo".to_string(),
                    capabilities: vec!["print".to_string(), "flexo".to_string()],
                    color_stations: None,
                },
            )
            .await
            .expect("rename apparatus with stable id");
        assert_eq!(stable_id, "apparatus:bobst 1");
        let renamed = ApparatusGroupService::new(Arc::new(reloaded))
            .apparatus_catalog("bobst 2", 20)
            .await
            .expect("load renamed apparatus");
        assert_eq!(renamed[0].id, "apparatus:bobst 1");
        assert_eq!(renamed[0].master.kind, "flexo");
    }
}
