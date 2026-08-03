use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rusqlite::{Connection, params};

use crate::core::calculate_materials::{
    CalculateMaterial, CalculateMaterialError, CalculateMaterialStorePort,
    CalculateMaterialUpsert, ensure_unique_name, merge_default_calculate_materials,
    normalize_material,
};

#[derive(Clone)]
pub struct CalculateMaterialStore {
    conn: Arc<Mutex<Connection>>,
}

impl CalculateMaterialStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self::open(path).unwrap_or_else(|error| {
            panic!("calculate material sqlite store unavailable: {error}");
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, CalculateMaterialError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| CalculateMaterialError::StoreFailed)?;
        }
        let conn = Connection::open(path).map_err(|_| CalculateMaterialError::StoreFailed)?;
        conn.busy_timeout(Duration::from_secs(5))
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS calculate_materials (
                id TEXT PRIMARY KEY,
                lower_name TEXT NOT NULL UNIQUE,
                payload_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_calculate_materials_name
                ON calculate_materials(lower_name);",
        )
        .map_err(|_| CalculateMaterialError::StoreFailed)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn list_overrides(&self) -> Result<Vec<CalculateMaterial>, CalculateMaterialError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        let mut stmt = conn
            .prepare("SELECT payload_json FROM calculate_materials ORDER BY lower_name")
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        let rows = stmt
            .query_map([], |row| {
                let payload: String = row.get(0)?;
                serde_json::from_str::<CalculateMaterial>(&payload)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))
            })
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| CalculateMaterialError::StoreFailed)
    }
}

#[async_trait]
impl CalculateMaterialStorePort for CalculateMaterialStore {
    async fn list(&self) -> Result<Vec<CalculateMaterial>, CalculateMaterialError> {
        Ok(merge_default_calculate_materials(self.list_overrides()?))
    }

    async fn upsert(
        &self,
        input: CalculateMaterialUpsert,
    ) -> Result<CalculateMaterial, CalculateMaterialError> {
        let material = normalize_material(input)?;
        let all = merge_default_calculate_materials(self.list_overrides()?);
        ensure_unique_name(&all, &material)?;
        let payload = serde_json::to_string(&material)
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        conn.execute(
            "INSERT INTO calculate_materials (id, lower_name, payload_json)
             VALUES (?1, lower(?2), ?3)
             ON CONFLICT(id) DO UPDATE SET
                lower_name = excluded.lower_name,
                payload_json = excluded.payload_json",
            params![material.id, material.name, payload],
        )
        .map_err(|_| CalculateMaterialError::StoreFailed)?;
        Ok(material)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn local_store_keeps_defaults_and_custom_materials() {
        let directory = tempdir().expect("tempdir");
        let store = CalculateMaterialStore::new(directory.path().join("materials.sqlite"));
        let before = store.list().await.expect("defaults");
        assert!(before.iter().any(|item| item.name == "PET"));

        let saved = store
            .upsert(CalculateMaterialUpsert {
                name: "BOPP custom".to_string(),
                variants: vec![crate::core::calculate_materials::CalculateMaterialVariant {
                    micron: 12,
                    coefficient: 1.25,
                    first_layer_coefficient: None,
                }],
                ..CalculateMaterialUpsert::default()
            })
            .await
            .expect("custom material");
        assert!(store
            .list()
            .await
            .expect("materials")
            .iter()
            .any(|item| item.id == saved.id));
    }
}
