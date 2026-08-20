use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::core::apparatus_groups::{
    ApparatusCatalogEntry, ApparatusGroup, ApparatusGroupError, ApparatusGroupStorePort,
    ApparatusMasterData, ApparatusSource,
};
use crate::core::apparatus_standard::{
    ApparatusClassification, ApparatusDisplayMetadata, ApparatusFamily, ApparatusId,
    ApparatusIdentity, ApparatusKind, CanonicalApparatus, CapabilityCode, CapabilityProfile,
    CapacityConfiguration, CatalogSource, OperationalPolicies, QueuePolicy, ToolingPolicy,
    TrainingReference, Versioning, aas_package_metadata_for_apparatus,
};

#[cfg(test)]
use crate::core::apparatus_groups::custom_apparatus_id;

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
                "SELECT id, name, payload_json,
                        row_number() OVER (ORDER BY lower_name, id) - 1
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
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
                ApparatusId::new(id.clone())
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
                let sort_order: i64 = row.get(3)?;
                Ok(ApparatusCatalogEntry {
                    id,
                    name,
                    source: ApparatusSource::Custom,
                    sort_order: sort_order.max(0) as usize,
                    master,
                })
            })
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| ApparatusGroupError::StoreFailed)
    }

    async fn canonical_apparatus_by_id(
        &self,
        apparatus_id: &ApparatusId,
    ) -> Result<Option<CanonicalApparatus>, ApparatusGroupError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let payload = conn
            .query_row(
                "SELECT payload_json FROM apparatus WHERE id = ?1",
                params![apparatus_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let Some(payload) = payload else {
            return Ok(None);
        };
        let payload = serde_json::from_str::<serde_json::Value>(&payload)
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let Some(canonical) = payload.get("canonical_apparatus") else {
            // Legacy master projections are not promoted at read time. A
            // missing canonical payload is a fail-closed unresolved lookup.
            return Ok(None);
        };
        let canonical = serde_json::from_value::<CanonicalApparatus>(canonical.clone())
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        if canonical.identity.id != *apparatus_id {
            return Err(ApparatusGroupError::InvalidApparatus);
        }
        canonical
            .validate()
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        Ok(Some(canonical))
    }

    async fn put_canonical_apparatus(
        &self,
        expected_revision: u64,
        apparatus: &CanonicalApparatus,
    ) -> Result<(), ApparatusGroupError> {
        apparatus
            .validate()
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        if apparatus.versioning.revision
            != expected_revision
                .checked_add(1)
                .ok_or(ApparatusGroupError::Conflict)?
        {
            return Err(ApparatusGroupError::Conflict);
        }
        let conn = self
            .conn
            .lock()
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let canonical_payload =
            serde_json::to_string(apparatus).map_err(|_| ApparatusGroupError::StoreFailed)?;
        let expected_revision = expected_revision.to_string();
        let changed = conn
            .execute(
                "UPDATE apparatus
                 SET payload_json = CASE
                     WHEN json_type(payload_json) = 'object' THEN
                         json_set(payload_json, '$.canonical_apparatus', json(?1))
                     ELSE
                         json_set('{}', '$.canonical_apparatus', json(?1))
                 END,
                     saved_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?2
                   AND CAST(json_extract(payload_json,
                       '$.canonical_apparatus.versioning.revision') AS TEXT) = ?3",
                params![
                    canonical_payload,
                    apparatus.identity.id.as_str(),
                    expected_revision
                ],
            )
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        if changed != 1 {
            return Err(ApparatusGroupError::Conflict);
        }
        Ok(())
    }

    async fn put_apparatus_with_canonical(
        &self,
        expected_revision: Option<u64>,
        requested_id: Option<&str>,
        name: &str,
        master: &ApparatusMasterData,
        canonical: &CanonicalApparatus,
    ) -> Result<String, ApparatusGroupError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApparatusGroupError::MissingApparatus);
        }
        canonical
            .validate()
            .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
        if let Some(expected_revision) = expected_revision
            && canonical.versioning.revision
                != expected_revision
                    .checked_add(1)
                    .ok_or(ApparatusGroupError::Conflict)?
        {
            return Err(ApparatusGroupError::Conflict);
        }
        let id = requested_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| {
                ApparatusId::new(id.to_string()).map_err(|_| ApparatusGroupError::InvalidApparatus)
            })
            .transpose()?
            .unwrap_or_else(|| canonical.identity.id.clone());
        if id != canonical.identity.id {
            return Err(ApparatusGroupError::InvalidApparatus);
        }

        let mut payload =
            serde_json::to_value(master).map_err(|_| ApparatusGroupError::StoreFailed)?;
        payload["canonical_apparatus"] =
            serde_json::to_value(canonical).map_err(|_| ApparatusGroupError::StoreFailed)?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let changed = if let Some(expected_revision) = expected_revision {
            let expected_revision = expected_revision.to_string();
            conn.execute(
                "UPDATE apparatus
                 SET lower_name = ?1, name = ?2, payload_json = ?3,
                     saved_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?4
                   AND CAST(json_extract(payload_json,
                       '$.canonical_apparatus.versioning.revision') AS TEXT) = ?5",
                params![
                    name.to_lowercase(),
                    name,
                    payload.to_string(),
                    id.as_str(),
                    expected_revision
                ],
            )
            .map_err(|_| ApparatusGroupError::StoreFailed)?
        } else {
            conn.execute(
                "INSERT INTO apparatus (id, lower_name, name, payload_json, saved_at)
                 VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 ON CONFLICT(id) DO NOTHING",
                params![id.as_str(), name.to_lowercase(), name, payload.to_string()],
            )
            .map_err(|_| ApparatusGroupError::StoreFailed)?
        };
        if changed != 1 {
            return Err(ApparatusGroupError::Conflict);
        }
        Ok(id.to_string())
    }
}

#[cfg(test)]
impl ApparatusGroupStore {
    pub(crate) async fn put_apparatus(&self, name: &str) -> Result<String, ApparatusGroupError> {
        self.save_apparatus(None, name, &ApparatusMasterData::default())
            .await
    }

    pub(crate) async fn put_apparatus_with_id(
        &self,
        requested_id: Option<&str>,
        name: &str,
        master: &ApparatusMasterData,
    ) -> Result<String, ApparatusGroupError> {
        self.save_apparatus(requested_id, name, master).await
    }

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
        let requested_id = requested_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| {
                ApparatusId::new(id.to_string()).map_err(|_| ApparatusGroupError::InvalidApparatus)
            })
            .transpose()?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| ApparatusGroupError::StoreFailed)?;
        let existing_id = if let Some(requested_id) = requested_id.as_ref() {
            conn.query_row(
                "SELECT id FROM apparatus WHERE id = ?1",
                params![requested_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
        } else {
            Ok(None)
        }
        .map_err(|_| ApparatusGroupError::StoreFailed)?;
        if let Some(existing_id) = existing_id {
            let payload =
                serde_json::to_string(master).map_err(|_| ApparatusGroupError::StoreFailed)?;
            let changed = conn
                .execute(
                    "UPDATE apparatus
                     SET lower_name = ?1, name = ?2,
                         payload_json = CASE
                             WHEN json_type(payload_json, '$.canonical_apparatus') IS NOT NULL THEN
                                 json_set(json(?3), '$.canonical_apparatus',
                                     json_extract(payload_json, '$.canonical_apparatus'))
                             ELSE json(?3)
                         END,
                         saved_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ?4",
                    params![name.to_lowercase(), name, payload, existing_id],
                )
                .map_err(|_| ApparatusGroupError::StoreFailed)?;
            if changed != 1 {
                return Err(ApparatusGroupError::MissingApparatus);
            }
            return Ok(existing_id);
        }
        let payload = serde_json::to_value(master).map_err(|_| ApparatusGroupError::StoreFailed)?;
        let id = requested_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| custom_apparatus_id(name));
        conn.execute(
            "INSERT INTO apparatus (id, lower_name, name, payload_json, saved_at)
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, name.to_lowercase(), name, payload.to_string()],
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
    let apparatus_row_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM apparatus",
        [],
        |row| row.get(0),
    )?;
    upgrade_legacy_apparatus_rows(conn)?;
    if apparatus_row_count == 0 {
        seed_default_apparatus_rows(conn)?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_apparatus_name
            ON apparatus(lower_name);
         CREATE UNIQUE INDEX IF NOT EXISTS idx_apparatus_id
            ON apparatus(id);",
    )
}

fn seed_default_apparatus_rows(conn: &Connection) -> rusqlite::Result<()> {
    for (sort_order, (id, name)) in [
        ("apparatus:default:bosma_7", "7 ta rangli bosma aparat"),
        ("apparatus:default:bosma_8", "8 ta rangli bosma aparat"),
        ("apparatus:default:bosma_9", "9 ta rangli bosma aparat"),
        ("apparatus:default:asset-004", "Extruder laminatsiya"),
        ("apparatus:default:asset-005", "Flexo pechat"),
        ("apparatus:default:holodniy_kley", "Holodniy kley aparat"),
        ("apparatus:default:asset-007", "Laminatsiya 1"),
        ("apparatus:default:asset-008", "Laminatsiya 2"),
        ("apparatus:default:paket", "Paket aparat"),
        ("apparatus:default:asset-010", "Rezka"),
    ]
    .into_iter()
    .enumerate()
    {
        let base_master = default_master_data(id)
            .ok_or_else(|| migration_error(format!("missing default apparatus master for {id}")))?;
        let base_master_json = serde_json::to_value(&base_master)
            .map_err(|_| migration_error("failed to serialize default apparatus master"))?;
        let master_object = base_master_json.as_object().cloned().unwrap_or_default();
        let master = legacy_master_data(id, name, &master_object);
        let canonical = canonical_from_master(id, name, &master, sort_order as u32)
            .map_err(|_| migration_error(format!("invalid default canonical apparatus {id}")))?;
        let mut payload = serde_json::to_value(&master)
            .map_err(|_| migration_error("failed to serialize default apparatus master"))?;
        payload["canonical_apparatus"] = serde_json::to_value(&canonical)
            .map_err(|_| migration_error("failed to serialize default canonical apparatus"))?;
        conn.execute(
            "INSERT INTO apparatus (id, lower_name, name, payload_json, saved_at)
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(lower_name) DO NOTHING",
            params![id, name.to_lowercase(), name, payload.to_string()],
        )?;
    }
    Ok(())
}

fn upgrade_legacy_apparatus_rows(conn: &Connection) -> rusqlite::Result<()> {
    let rows = {
        let mut stmt = conn.prepare(
            "SELECT rowid, id, lower_name, name, payload_json
             FROM apparatus
             ORDER BY rowid ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let mut canonical_ids = BTreeMap::<String, i64>::new();
    let mut aliases = BTreeMap::<String, String>::new();
    for (rowid, old_id, lower_name, name, payload_json) in rows {
        let new_id = upgraded_apparatus_id(old_id.as_deref(), &name, &lower_name);
        if let Some(previous_rowid) = canonical_ids.insert(new_id.clone(), rowid)
            && previous_rowid != rowid
        {
            return Err(migration_error(format!(
                "duplicate deterministic canonical apparatus id {new_id}"
            )));
        }

        let payload = payload_json
            .as_deref()
            .map(serde_json::from_str::<serde_json::Value>)
            .transpose()
            .map_err(|_| migration_error("apparatus payload_json is not valid JSON"))?
            .unwrap_or_else(|| serde_json::json!({}));
        let payload_object = payload.as_object().cloned().unwrap_or_default();
        let master = legacy_master_data(&new_id, &name, &payload_object);
        let sort_order = payload_object
            .get("sort_order")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default()
            .min(u32::MAX as u64) as u32;
        let generated = canonical_from_master(&new_id, &name, &master, sort_order)
            .map_err(|_| migration_error(format!("invalid canonical master for {new_id}")))?;
        let generated = serde_json::to_value(generated)
            .map_err(|_| migration_error("failed to serialize canonical apparatus"))?;
        let canonical = merge_canonical_payload(
            generated,
            payload_object.get("canonical_apparatus"),
            &new_id,
        )?;
        let canonical = serde_json::from_value::<CanonicalApparatus>(canonical.clone())
            .map_err(|_| migration_error(format!("invalid canonical payload for {new_id}")))?;
        canonical.validate().map_err(|_| {
            migration_error(format!("canonical payload failed validation for {new_id}"))
        })?;

        let mut upgraded_payload = payload_object;
        upgraded_payload.insert(
            "canonical_apparatus".to_string(),
            serde_json::to_value(&canonical)
                .map_err(|_| migration_error("failed to serialize canonical apparatus"))?,
        );
        conn.execute(
            "UPDATE apparatus
             SET id = ?1, payload_json = ?2
             WHERE rowid = ?3",
            params![
                new_id,
                serde_json::Value::Object(upgraded_payload).to_string(),
                rowid
            ],
        )?;

        insert_alias(&mut aliases, old_id.as_deref(), &new_id)?;
        insert_alias(&mut aliases, Some(&name), &new_id)?;
        insert_alias(&mut aliases, Some(&lower_name), &new_id)?;
        insert_alias(&mut aliases, Some(&new_id), &new_id)?;
    }

    upgrade_group_payloads(conn, &aliases)?;
    Ok(())
}

fn upgrade_group_payloads(
    conn: &Connection,
    aliases: &BTreeMap<String, String>,
) -> rusqlite::Result<()> {
    let rows = {
        let mut stmt = conn.prepare("SELECT rowid, payload_json FROM apparatus_groups")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (rowid, raw_payload) in rows {
        let mut payload = serde_json::from_str::<serde_json::Value>(&raw_payload)
            .map_err(|_| migration_error("apparatus group payload_json is not valid JSON"))?;
        let Some(apparatus) = payload
            .get_mut("apparatus")
            .and_then(|value| value.as_array_mut())
        else {
            continue;
        };
        for value in apparatus {
            let Some(legacy_value) = value.as_str() else {
                return Err(migration_error(
                    "apparatus group contains a non-string apparatus identity",
                ));
            };
            let canonical_id = aliases.get(&legacy_key(legacy_value)).ok_or_else(|| {
                migration_error(format!(
                    "apparatus group references unknown legacy apparatus {legacy_value}"
                ))
            })?;
            *value = serde_json::Value::String(canonical_id.clone());
        }
        conn.execute(
            "UPDATE apparatus_groups SET payload_json = ?1 WHERE rowid = ?2",
            params![payload.to_string(), rowid],
        )?;
    }
    Ok(())
}

fn insert_alias(
    aliases: &mut BTreeMap<String, String>,
    value: Option<&str>,
    canonical_id: &str,
) -> rusqlite::Result<()> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let key = legacy_key(value);
    if let Some(previous) = aliases.insert(key, canonical_id.to_string())
        && previous != canonical_id
    {
        return Err(migration_error(format!(
            "ambiguous legacy apparatus mapping for {value}"
        )));
    }
    Ok(())
}

fn legacy_key(value: &str) -> String {
    value.trim().to_lowercase()
}

fn upgraded_apparatus_id(id: Option<&str>, name: &str, lower_name: &str) -> String {
    if let Some(default_id) = default_apparatus_id(id, name) {
        return default_id.to_string();
    }
    if let Some(id) = id.map(str::trim).filter(|id| !id.is_empty())
        && is_safe_canonical_id(id, name)
    {
        return id.to_string();
    }
    let seed = id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| format!("id:{id}"))
        .unwrap_or_else(|| format!("name:{lower_name}"));
    let digest = Sha256::digest(format!("mini-rs-erp/apparatus-legacy-id/v1:{seed}").as_bytes());
    format!("apparatus:legacy:{}", HEXLOWER.encode(&digest))
}

fn default_apparatus_id(id: Option<&str>, name: &str) -> Option<&'static str> {
    let id = id.map(legacy_key);
    let name = legacy_key(name);
    match (id.as_deref(), name.as_str()) {
        (Some("apparatus:default:bosma_7"), _)
        | (_, "7 ta rangli bosma aparat" | "7 ta rangli bosma" | "7 ta rangli pechat") => {
            Some("apparatus:default:bosma_7")
        }
        (Some("apparatus:default:bosma_8"), _)
        | (_, "8 ta rangli bosma aparat" | "8 ta rangli bosma" | "8 ta rangli pechat") => {
            Some("apparatus:default:bosma_8")
        }
        (Some("apparatus:default:bosma_9"), _)
        | (_, "9 ta rangli bosma aparat" | "9 ta rangli bosma" | "9 ta rangli pechat") => {
            Some("apparatus:default:bosma_9")
        }
        (Some("apparatus:default:extruder_laminatsiya"), _) | (_, "extruder laminatsiya") => {
            Some("apparatus:default:asset-004")
        }
        (Some("apparatus:default:flexo_pechat"), _) | (_, "flexo pechat") => {
            Some("apparatus:default:asset-005")
        }
        (Some("apparatus:default:holodniy_kley"), _)
        | (_, "holodniy kley aparat" | "holodniy kley") => Some("apparatus:default:holodniy_kley"),
        (Some("apparatus:default:laminatsiya_1"), _) | (_, "laminatsiya 1") => {
            Some("apparatus:default:asset-007")
        }
        (Some("apparatus:default:laminatsiya_2"), _) | (_, "laminatsiya 2") => {
            Some("apparatus:default:asset-008")
        }
        (Some("apparatus:default:paket"), _) | (_, "paket aparat" | "paket") => {
            Some("apparatus:default:paket")
        }
        (Some("apparatus:default:rezka"), _) | (_, "rezka apparat" | "rezka") => {
            Some("apparatus:default:asset-010")
        }
        _ => None,
    }
}

fn is_safe_canonical_id(id: &str, name: &str) -> bool {
    let Ok(id) = ApparatusId::new(id.to_string()) else {
        return false;
    };
    let title_key = title_identity_key(name);
    let id_key = id.as_str().rsplit(':').next().unwrap_or_default();
    title_key
        != id_key
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
}

fn title_identity_key(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn legacy_master_data(
    id: &str,
    name: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> ApparatusMasterData {
    let mut master =
        serde_json::from_value::<ApparatusMasterData>(serde_json::Value::Object(payload.clone()))
            .unwrap_or_default();
    if let Some(default_master) = default_master_data(id) {
        let factory_map_object_id = master.factory_map_object_id.clone();
        let training_enabled = master.training_enabled;
        let tooling_policy = master.tooling_policy;
        master = default_master;
        master.factory_map_object_id = factory_map_object_id;
        master.training_enabled = training_enabled;
        master.tooling_policy = tooling_policy;
    }
    if master.family.trim().is_empty() {
        master.family = "other".to_string();
    }
    if master.kind.trim().is_empty() {
        master.kind = "other".to_string();
    }
    if master.capabilities.is_empty() {
        master.capabilities = default_capabilities_for_kind(&master.kind);
    }
    if master.capability_profiles.is_empty() {
        master.capability_profiles = master
            .capabilities
            .iter()
            .map(
                |code| crate::core::apparatus_groups::ApparatusCapabilityProfile {
                    code: code.clone(),
                    level: 1,
                    valid_from_unix: None,
                    valid_to_unix: None,
                    enabled: true,
                },
            )
            .collect();
    }
    if master.capacity.is_none() {
        master.capacity = Some(default_capacity());
    }
    let _ = name;
    master
}

fn default_master_data(id: &str) -> Option<ApparatusMasterData> {
    let (family, kind, capabilities, color_stations) = match id {
        "apparatus:default:bosma_7" => ("pechat", "color_pechat", vec!["print", "pechat"], Some(7)),
        "apparatus:default:bosma_8" => ("pechat", "color_pechat", vec!["print", "pechat"], Some(8)),
        "apparatus:default:bosma_9" => ("pechat", "color_pechat", vec!["print", "pechat"], Some(9)),
        "apparatus:default:asset-004" => (
            "laminatsiya",
            "extruder_laminatsiya",
            vec!["laminate"],
            None,
        ),
        "apparatus:default:asset-005" => {
            ("pechat", "flexo", vec!["print", "pechat", "flexo"], None)
        }
        "apparatus:default:holodniy_kley" => ("kley", "holodniy_kley", vec!["glue"], None),
        "apparatus:default:asset-007" | "apparatus:default:asset-008" => {
            ("laminatsiya", "laminatsiya", vec!["laminate"], None)
        }
        "apparatus:default:paket" => ("paket", "paket", vec!["package"], None),
        "apparatus:default:asset-010" => ("rezka", "rezka", vec!["cut"], None),
        _ => return None,
    };
    Some(ApparatusMasterData {
        family: family.to_string(),
        kind: kind.to_string(),
        capabilities: capabilities.into_iter().map(str::to_string).collect(),
        capability_profiles: Vec::new(),
        color_stations,
        factory_map_object_id: None,
        training_enabled: false,
        tooling_policy: None,
        capacity: Some(default_capacity()),
    })
}

fn default_capabilities_for_kind(kind: &str) -> Vec<String> {
    match kind {
        "color_pechat" => vec!["print", "pechat"],
        "flexo" => vec!["print", "pechat", "flexo"],
        "laminatsiya" | "extruder_laminatsiya" => vec!["laminate"],
        "rezka" => vec!["cut"],
        "paket" => vec!["package"],
        "holodniy_kley" => vec!["glue"],
        _ => vec!["apparatus"],
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn default_capacity() -> CapacityConfiguration {
    CapacityConfiguration {
        capacity_slots: 1,
        setup_minutes: 0,
        cleanup_minutes: 0,
        efficiency_percent: 100,
        finite_capacity: true,
        working_windows: Vec::new(),
    }
}

fn canonical_from_master(
    id: &str,
    name: &str,
    master: &ApparatusMasterData,
    catalog_order: u32,
) -> Result<CanonicalApparatus, ApparatusGroupError> {
    let id = ApparatusId::new(id.to_string()).map_err(|_| ApparatusGroupError::InvalidApparatus)?;
    let family = match master.family.as_str() {
        "pechat" => ApparatusFamily::Pechat,
        "laminatsiya" => ApparatusFamily::Laminatsiya,
        "rezka" => ApparatusFamily::Rezka,
        "paket" => ApparatusFamily::Paket,
        "kley" => ApparatusFamily::Kley,
        "other" => ApparatusFamily::Other,
        _ => return Err(ApparatusGroupError::InvalidFamily),
    };
    let kind = match master.kind.as_str() {
        "color_pechat" => ApparatusKind::ColorPechat,
        "flexo" => ApparatusKind::Flexo,
        "laminatsiya" => ApparatusKind::Laminatsiya,
        "extruder_laminatsiya" => ApparatusKind::ExtruderLaminatsiya,
        "rezka" => ApparatusKind::Rezka,
        "paket" => ApparatusKind::Paket,
        "holodniy_kley" => ApparatusKind::HolodniyKley,
        "other" => ApparatusKind::Other,
        _ => return Err(ApparatusGroupError::InvalidKind),
    };
    let capabilities = master
        .capabilities
        .iter()
        .map(|code| capability_code(code))
        .collect::<Result<Vec<_>, _>>()?;
    let capability_profiles = master
        .capability_profiles
        .iter()
        .map(|profile| {
            Ok(CapabilityProfile {
                code: capability_code(&profile.code)?,
                level: profile.level,
                valid_from_unix: profile.valid_from_unix,
                valid_to_unix: profile.valid_to_unix,
                enabled: profile.enabled,
            })
        })
        .collect::<Result<Vec<_>, ApparatusGroupError>>()?;
    let tooling = master.tooling_policy.unwrap_or_else(|| {
        if family == ApparatusFamily::Pechat {
            ToolingPolicy::QolipScanRequired
        } else {
            ToolingPolicy::QolipScanNotRequired
        }
    });
    let source = canonical_source(&id);
    let canonical = CanonicalApparatus {
        identity: ApparatusIdentity {
            id: id.clone(),
            display: ApparatusDisplayMetadata {
                display_name: name.to_string(),
                description: String::new(),
                catalog_order,
            },
        },
        classification: ApparatusClassification {
            family,
            kind,
            color_stations: master.color_stations,
        },
        capabilities,
        capability_profiles,
        policies: OperationalPolicies {
            queue: QueuePolicy::StrictSequence,
            material: Default::default(),
            tooling,
        },
        capacity: master
            .capacity
            .clone()
            .ok_or(ApparatusGroupError::InvalidApparatus)?,
        placement: master
            .factory_map_object_id
            .clone()
            .map(
                |factory_map_object_id| crate::core::apparatus_standard::PlacementReference {
                    factory_map_object_id,
                },
            ),
        training: TrainingReference {
            enabled: master.training_enabled,
        },
        provenance: crate::core::apparatus_standard::Provenance {
            source: if source {
                CatalogSource::Default
            } else {
                CatalogSource::Custom
            },
            source_ref: None,
        },
        versioning: Versioning { revision: 1 },
        aas: aas_package_metadata_for_apparatus(&id),
    };
    canonical
        .validate()
        .map_err(|_| ApparatusGroupError::InvalidApparatus)?;
    Ok(canonical)
}

fn canonical_source(id: &ApparatusId) -> bool {
    id.as_str().starts_with("apparatus:default:")
}

fn capability_code(value: &str) -> Result<CapabilityCode, ApparatusGroupError> {
    match value {
        "print" => Ok(CapabilityCode::Print),
        "pechat" => Ok(CapabilityCode::Pechat),
        "flexo" => Ok(CapabilityCode::Flexo),
        "laminate" => Ok(CapabilityCode::Laminate),
        "cut" => Ok(CapabilityCode::Cut),
        "package" => Ok(CapabilityCode::Package),
        "glue" => Ok(CapabilityCode::Glue),
        "apparatus" => Ok(CapabilityCode::Apparatus),
        _ => Err(ApparatusGroupError::InvalidCapability),
    }
}

fn merge_canonical_payload(
    generated: serde_json::Value,
    existing: Option<&serde_json::Value>,
    id: &str,
) -> rusqlite::Result<serde_json::Value> {
    let generated_object = generated
        .as_object()
        .cloned()
        .ok_or_else(|| migration_error("generated canonical payload is not an object"))?;
    let Some(existing_object) = existing
        .filter(|value| value.is_object())
        .and_then(serde_json::Value::as_object)
    else {
        return Ok(serde_json::Value::Object(generated_object));
    };

    let mut merged = generated_object.clone();
    for (key, value) in existing_object {
        if key != "identity" && key != "policies" {
            merged.insert(key.clone(), value.clone());
        }
    }

    let generated_identity = generated_object
        .get("identity")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .ok_or_else(|| migration_error("generated canonical identity is missing"))?;
    let mut identity = generated_identity.clone();
    let mut display = generated_identity
        .get("display")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .ok_or_else(|| migration_error("generated canonical display is missing"))?;
    if let Some(existing_display) = existing_object
        .get("identity")
        .and_then(|value| value.get("display"))
        .and_then(serde_json::Value::as_object)
    {
        // display_name is generated from the migrated master and id is fixed
        // below.  Only optional display metadata may come from a partial
        // legacy canonical blob.
        if let Some(description) = existing_display.get("description")
            && description.is_string()
        {
            display.insert("description".to_string(), description.clone());
        }
        if let Some(catalog_order) = existing_display.get("catalog_order")
            && catalog_order
                .as_u64()
                .is_some_and(|value| value <= u32::MAX as u64)
        {
            display.insert("catalog_order".to_string(), catalog_order.clone());
        }
    }
    identity.insert("display".to_string(), serde_json::Value::Object(display));
    identity.insert("id".to_string(), serde_json::Value::String(id.to_string()));
    merged.insert(
        "identity".to_string(),
        serde_json::Value::Object(identity.clone()),
    );

    let generated_policies = generated_object
        .get("policies")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .ok_or_else(|| migration_error("generated canonical policies are missing"))?;
    let mut policies = generated_policies;
    if let Some(existing_policies) = existing_object
        .get("policies")
        .and_then(|value| value.as_object())
    {
        policies.extend(existing_policies.clone());
        let mut material = generated_object
            .get("policies")
            .and_then(|value| value.get("material"))
            .and_then(serde_json::Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(existing_material) = existing_policies
            .get("material")
            .and_then(|value| value.as_object())
        {
            material.extend(existing_material.clone());
        }
        policies.insert("material".to_string(), serde_json::Value::Object(material));
    }
    merged.insert("policies".to_string(), serde_json::Value::Object(policies));
    let merged = serde_json::Value::Object(merged);
    if canonical_payload_is_valid(&merged) {
        Ok(merged)
    } else {
        // A partial or stale nested config may not make the cutover produce an
        // invalid live record.  Keep generated required fields and discard
        // only the invalid legacy overlay.
        let mut fallback = generated_object;
        fallback.insert("identity".to_string(), serde_json::Value::Object(identity));
        Ok(serde_json::Value::Object(fallback))
    }
}

fn canonical_payload_is_valid(value: &serde_json::Value) -> bool {
    serde_json::from_value::<CanonicalApparatus>(value.clone())
        .ok()
        .is_some_and(|canonical| canonical.validate().is_ok())
}

fn migration_error(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message.into(),
    )))
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
        ApparatusGroupService, ApparatusGroupStorePort, ApparatusGroupUpsert, ApparatusMasterData,
        ApparatusUpsert,
    };
    use crate::core::apparatus_standard::ApparatusId;

    #[tokio::test]
    async fn apparatus_group_store_persists_groups_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("apparatus_groups.sqlite");
        let service = ApparatusGroupService::new(Arc::new(
            ApparatusGroupStore::open(&path).expect("open apparatus group store"),
        ));
        for (id, name) in [
            ("apparatus:default:bosma_7", "7 ta rangli bosma aparat"),
            ("apparatus:default:bosma_8", "8 ta rangli bosma aparat"),
            ("apparatus:default:bosma_9", "9 ta rangli bosma aparat"),
            ("apparatus:default:asset-004", "Extruder laminatsiya"),
            ("apparatus:default:asset-005", "Flexo pechat"),
            ("apparatus:default:holodniy_kley", "Holodniy kley aparat"),
            ("apparatus:default:asset-007", "Laminatsiya 1"),
            ("apparatus:default:asset-008", "Laminatsiya 2"),
            ("apparatus:default:paket", "Paket aparat"),
            ("apparatus:default:asset-010", "Rezka"),
        ] {
            service
                .upsert_apparatus(ApparatusUpsert {
                    id: Some(id.to_string()),
                    name: name.to_string(),
                    master: ApparatusMasterData::default(),
                })
                .await
                .expect("seed canonical default apparatus");
        }

        let saved = service
            .upsert_group(ApparatusGroupUpsert {
                name: " pechat ".to_string(),
                apparatus: vec![
                    "apparatus:default:bosma_7".to_string(),
                    "apparatus:default:bosma_8".to_string(),
                    "apparatus:default:bosma_7".to_string(),
                ],
            })
            .await
            .expect("save apparatus group");
        assert_eq!(saved.name, "Bosma aparat");
        assert_eq!(
            saved.apparatus,
            vec![
                "apparatus:default:bosma_7".to_string(),
                "apparatus:default:bosma_8".to_string(),
                "apparatus:default:bosma_9".to_string(),
                "apparatus:default:asset-005".to_string(),
            ]
        );

        let reloaded = ApparatusGroupStore::open(&path).expect("reopen apparatus group store");
        let groups = reloaded.groups().await.expect("load apparatus groups");

        assert_eq!(groups, vec![saved]);

        let reloaded_service = ApparatusGroupService::new(Arc::new(reloaded.clone()));
        let created = reloaded_service
            .upsert_apparatus(ApparatusUpsert {
                id: None,
                name: "Bobst 1".to_string(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("save canonical apparatus")
            .id;
        assert!(created.starts_with("apparatus:custom:"));
        assert_eq!(
            reloaded.apparatus("bob", 20).await.expect("list apparatus"),
            vec!["Bobst 1".to_string()]
        );
        let catalog = reloaded_service
            .apparatus_catalog("bob", 20)
            .await
            .expect("load apparatus metadata");
        assert_eq!(catalog[0].master.family, "other");
        assert_eq!(catalog[0].master.kind, "other");
        assert_eq!(catalog[0].master.capabilities, vec!["apparatus"]);

        let stable_id = reloaded_service
            .upsert_apparatus(ApparatusUpsert {
                id: Some(created.clone()),
                name: "Bobst 2".to_string(),
                master: ApparatusMasterData {
                    family: "pechat".to_string(),
                    kind: "flexo".to_string(),
                    capabilities: vec!["print".to_string(), "flexo".to_string()],
                    capability_profiles: Vec::new(),
                    color_stations: None,
                    factory_map_object_id: None,
                    training_enabled: false,
                    tooling_policy: Some(
                        crate::core::apparatus_standard::ToolingPolicy::QolipScanNotRequired,
                    ),
                    capacity: Some(crate::core::apparatus_standard::CapacityConfiguration {
                        capacity_slots: 1,
                        setup_minutes: 0,
                        cleanup_minutes: 0,
                        efficiency_percent: 100,
                        finite_capacity: true,
                        working_windows: Vec::new(),
                    }),
                },
            })
            .await
            .expect("rename canonical apparatus")
            .id;
        assert_eq!(stable_id, created);
        let renamed = reloaded_service
            .apparatus_catalog("bobst 2", 20)
            .await
            .expect("load renamed apparatus");
        assert_eq!(renamed[0].id, stable_id);
        assert_eq!(renamed[0].master.kind, "flexo");
    }

    #[tokio::test]
    async fn apparatus_group_store_persists_canonical_payload_across_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("apparatus_groups.sqlite");
        let service = ApparatusGroupService::new(Arc::new(
            ApparatusGroupStore::open(&path).expect("open apparatus group store"),
        ));
        let saved = service
            .upsert_apparatus(ApparatusUpsert {
                id: Some("apparatus:custom:opaque-001".to_string()),
                name: "Durable press".to_string(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("save canonical apparatus");

        let reloaded = ApparatusGroupService::new(Arc::new(
            ApparatusGroupStore::open(&path).expect("reopen apparatus group store"),
        ));
        let id = ApparatusId::new(saved.id).expect("canonical id");
        let canonical = reloaded
            .canonical_apparatus_by_id(&id)
            .await
            .expect("load canonical apparatus")
            .expect("canonical payload persisted");
        assert_eq!(canonical.identity.id, id);
        assert_eq!(canonical.identity.display.display_name, "Durable press");
        assert_eq!(
            canonical.aas.submodel_id,
            "urn:mini-rs-erp:submodel:apparatus:custom:opaque-001"
        );
    }

    #[tokio::test]
    async fn legacy_rows_and_group_references_are_backfilled_with_valid_opaque_identity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("legacy_apparatus.sqlite");
        {
            let conn = Connection::open(&path).expect("open legacy database");
            conn.execute_batch(
                r#"CREATE TABLE apparatus_groups (
                    lower_name TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    saved_at TEXT NOT NULL
                );
                CREATE TABLE apparatus (
                    lower_name TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    saved_at TEXT NOT NULL
                );
                INSERT INTO apparatus (lower_name, name, payload_json, saved_at)
                VALUES (
                    'legacy press',
                    'Legacy press',
                    '{
                        "family":"other",
                        "kind":"other",
                        "capabilities":["apparatus"],
                        "canonical_apparatus": {
                            "identity": {
                                "id":"apparatus:legacy:stale",
                                "display":{"display_name":""}
                            }
                        }
                    }',
                    '2026-01-01T00:00:00Z'
                );
                INSERT INTO apparatus_groups (lower_name, name, payload_json, saved_at)
                VALUES (
                    'legacy group',
                    'Legacy group',
                    '{"name":"Legacy group","apparatus":["Legacy press"]}',
                    '2026-01-01T00:00:00Z'
                );"#,
            )
            .expect("seed legacy database");
        }

        let store = ApparatusGroupStore::open(&path).expect("upgrade legacy database");
        let catalog = store
            .apparatus_catalog("", 10)
            .await
            .expect("load upgraded catalog");
        assert_eq!(catalog.len(), 1);
        let id = ApparatusId::new(catalog[0].id.clone()).expect("opaque canonical id");
        assert!(id.as_str().starts_with("apparatus:legacy:"));
        assert_eq!(id.as_str().matches(':').count(), 2);

        let canonical = store
            .canonical_apparatus_by_id(&id)
            .await
            .expect("load upgraded canonical")
            .expect("canonical payload was materialized");
        assert_eq!(canonical.identity.id, id);
        assert_eq!(canonical.identity.display.display_name, "Legacy press");

        let groups = store.groups().await.expect("load upgraded group");
        assert_eq!(groups[0].apparatus, vec![catalog[0].id.clone()]);
    }

    #[tokio::test]
    async fn legacy_update_preserves_canonical_sidecar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("apparatus_groups.sqlite");
        let store = ApparatusGroupStore::open(&path).expect("open apparatus group store");
        let service = ApparatusGroupService::new(Arc::new(store.clone()));
        let saved = service
            .upsert_apparatus(ApparatusUpsert {
                id: Some("apparatus:custom:sidecar-001".to_string()),
                name: "Sidecar press".to_string(),
                master: ApparatusMasterData::default(),
            })
            .await
            .expect("save canonical apparatus");

        store
            .put_apparatus_with_id(
                Some(&saved.id),
                "Compatibility rename",
                &ApparatusMasterData::default(),
            )
            .await
            .expect("write legacy projection");

        let id = ApparatusId::new(saved.id).expect("canonical id");
        let canonical = store
            .canonical_apparatus_by_id(&id)
            .await
            .expect("load canonical sidecar")
            .expect("legacy write preserved canonical sidecar");
        assert_eq!(canonical.identity.id, id);
        assert_eq!(canonical.identity.display.display_name, "Sidecar press");
    }
}
