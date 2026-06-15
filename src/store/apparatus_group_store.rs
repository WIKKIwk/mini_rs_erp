use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};

use crate::core::apparatus_groups::{ApparatusGroup, ApparatusGroupError, ApparatusGroupStorePort};

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

    async fn put_apparatus(&self, name: &str) -> Result<String, ApparatusGroupError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        let lower_name = name.to_lowercase();
        let conn = self
            .conn
            .lock()
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        conn.execute(
            "INSERT INTO apparatus (lower_name, name, saved_at)
             VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(lower_name) DO UPDATE SET
               name = excluded.name,
               saved_at = excluded.saved_at",
            params![lower_name, name],
        )
        .map_err(|_| ApparatusGroupError::StoreFailed)?;
        conn.query_row(
            "SELECT name FROM apparatus WHERE lower_name = ?1",
            params![name.to_lowercase()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| ApparatusGroupError::StoreFailed)?
        .ok_or(ApparatusGroupError::StoreFailed)
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
            lower_name TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            saved_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_apparatus_name
            ON apparatus(lower_name);",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::apparatus_groups::{ApparatusGroupService, ApparatusGroupUpsert};

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
        assert_eq!(saved.name, "pechat");
        assert_eq!(
            saved.apparatus,
            vec![
                "7 ta rangli pechat".to_string(),
                "8 ta rangli pechat".to_string(),
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
    }
}
