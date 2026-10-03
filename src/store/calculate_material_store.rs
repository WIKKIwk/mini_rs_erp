use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rusqlite::{Connection, TransactionBehavior, params};

use crate::core::calculate_materials::{
    CalculateMaterial, CalculateMaterialError, CalculateMaterialStorePort, CalculateMaterialUpsert,
    merge_default_calculate_materials, normalize_material, prepare_material_upsert,
    reorder_calculate_materials,
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

    fn list_on(conn: &Connection) -> Result<Vec<CalculateMaterial>, CalculateMaterialError> {
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
        let overrides = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        Ok(merge_default_calculate_materials(overrides))
    }

    fn save_on(
        conn: &Connection,
        material: &CalculateMaterial,
    ) -> Result<(), CalculateMaterialError> {
        let payload =
            serde_json::to_string(material).map_err(|_| CalculateMaterialError::StoreFailed)?;
        conn.execute(
            "INSERT INTO calculate_materials (id, lower_name, payload_json)
             VALUES (?1, lower(?2), ?3)
             ON CONFLICT(id) DO UPDATE SET
                lower_name = excluded.lower_name,
                payload_json = excluded.payload_json",
            params![material.id, material.name, payload],
        )
        .map_err(|_| CalculateMaterialError::StoreFailed)?;
        Ok(())
    }
}

#[async_trait]
impl CalculateMaterialStorePort for CalculateMaterialStore {
    async fn list(&self) -> Result<Vec<CalculateMaterial>, CalculateMaterialError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        Self::list_on(&conn)
    }

    async fn upsert(
        &self,
        input: CalculateMaterialUpsert,
    ) -> Result<CalculateMaterial, CalculateMaterialError> {
        let mut material = normalize_material(input)?;
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        let all = Self::list_on(&tx)?;
        if let Some(pinned) = prepare_material_upsert(&all, &mut material)? {
            for item in &pinned {
                Self::save_on(&tx, item)?;
            }
        } else {
            Self::save_on(&tx, &material)?;
        }
        tx.commit()
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        Ok(material)
    }

    async fn reorder(
        &self,
        material_ids: Vec<String>,
    ) -> Result<Vec<CalculateMaterial>, CalculateMaterialError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        let materials = reorder_calculate_materials(Self::list_on(&tx)?, &material_ids)?;
        for material in &materials {
            Self::save_on(&tx, material)?;
        }
        tx.commit()
            .map_err(|_| CalculateMaterialError::StoreFailed)?;
        Ok(materials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn sequence_is_persisted_and_failed_writes_roll_back() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("sequence.sqlite");
        let store = CalculateMaterialStore::new(&path);
        let ids: Vec<_> = store
            .list()
            .await
            .unwrap()
            .iter()
            .rev()
            .map(|item| item.id.clone())
            .collect();
        let saved = store.reorder(ids.clone()).await.unwrap();
        drop(store);
        let store = CalculateMaterialStore::new(&path);
        assert_eq!(store.list().await.unwrap(), saved);
        let edited = store
            .upsert(CalculateMaterialUpsert {
                id: ids[1].clone(),
                name: "ZZZ custom name".into(),
                density_g_cm3: 0.92,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(edited.sort_order, Some(1));
        assert_eq!(store.list().await.unwrap()[1].id, ids[1]);
        let before_failure = store.list().await.unwrap();
        // The second write fails, after the first row has already been updated.
        store
            .conn
            .lock()
            .unwrap()
            .execute_batch(&format!(
                "CREATE TRIGGER fail_sequence BEFORE UPDATE ON calculate_materials
             WHEN OLD.id = '{}' BEGIN SELECT RAISE(ABORT, 'write failed'); END;",
                ids[ids.len() - 2]
            ))
            .unwrap();
        assert!(
            store
                .reorder(ids.into_iter().rev().collect())
                .await
                .is_err()
        );
        assert_eq!(store.list().await.unwrap(), before_failure);
    }

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
                    actual_gsm: None,
                }],
                density_g_cm3: 0.91,
                ..CalculateMaterialUpsert::default()
            })
            .await
            .expect("custom material");
        assert!(
            store
                .list()
                .await
                .expect("materials")
                .iter()
                .any(|item| item.id == saved.id)
        );
    }
}
