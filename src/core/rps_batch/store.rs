use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use heed::types::Bytes;
use heed::{BoxedError, BytesDecode, BytesEncode, Database, Env, EnvOpenOptions};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::models::RpsBatchSession;
use super::ports::{RpsBatchStoreError, RpsBatchStorePort};
use crate::error::AppError;

pub struct RpsBatchLmdbStore {
    env: Env,
    db: Database<Bytes, RpsBatchSessionCodec>,
    write_lock: Arc<Mutex<()>>,
}

struct RpsBatchSessionCodec;

const RPS_BATCH_MAGIC: &[u8] = b"RPSB1";

impl<'a> BytesEncode<'a> for RpsBatchSessionCodec {
    type EItem = RpsBatchSession;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let payload = bincode::serialize(item)?;
        let mut bytes = Vec::with_capacity(RPS_BATCH_MAGIC.len() + payload.len());
        bytes.extend_from_slice(RPS_BATCH_MAGIC);
        bytes.extend_from_slice(&payload);
        Ok(Cow::Owned(bytes))
    }
}

impl<'a> BytesDecode<'a> for RpsBatchSessionCodec {
    type DItem = RpsBatchSession;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if let Some(payload) = bytes.strip_prefix(RPS_BATCH_MAGIC) {
            return Ok(match bincode::deserialize(payload) {
                Ok(batch) => batch,
                Err(_) => match bincode::deserialize::<RpsBatchSessionV2>(payload) {
                    Ok(batch) => batch.into(),
                    Err(_) => bincode::deserialize::<RpsBatchSessionV1>(payload)?.into(),
                },
            });
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct RpsBatchSessionV2 {
    id: String,
    active: bool,
    owner_key: String,
    owner_role: String,
    owner_ref: String,
    driver_url: String,
    item_code: String,
    item_name: String,
    warehouse: String,
    printer: String,
    print_mode: String,
    quantity_source: String,
    manual_qty_kg: f64,
    tare_enabled: bool,
    tare_kg: f64,
    last_error: String,
    last_error_at: String,
    created_at: String,
    updated_at: String,
}

impl From<RpsBatchSessionV2> for RpsBatchSession {
    fn from(batch: RpsBatchSessionV2) -> Self {
        Self {
            id: batch.id,
            active: batch.active,
            owner_key: batch.owner_key,
            owner_role: batch.owner_role,
            owner_ref: batch.owner_ref,
            driver_url: batch.driver_url,
            item_code: batch.item_code,
            item_name: batch.item_name,
            warehouse: batch.warehouse,
            printer: batch.printer,
            print_mode: batch.print_mode,
            quantity_source: batch.quantity_source,
            manual_qty_kg: batch.manual_qty_kg,
            tare_enabled: batch.tare_enabled,
            tare_kg: batch.tare_kg,
            last_error: batch.last_error,
            last_error_at: batch.last_error_at,
            created_at: batch.created_at,
            updated_at: batch.updated_at,
            ..RpsBatchSession::default()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct RpsBatchSessionV1 {
    id: String,
    active: bool,
    owner_key: String,
    owner_role: String,
    owner_ref: String,
    driver_url: String,
    item_code: String,
    item_name: String,
    warehouse: String,
    printer: String,
    print_mode: String,
    quantity_source: String,
    manual_qty_kg: f64,
    tare_enabled: bool,
    tare_kg: f64,
    created_at: String,
    updated_at: String,
}

impl From<RpsBatchSessionV1> for RpsBatchSession {
    fn from(batch: RpsBatchSessionV1) -> Self {
        Self {
            id: batch.id,
            active: batch.active,
            owner_key: batch.owner_key,
            owner_role: batch.owner_role,
            owner_ref: batch.owner_ref,
            driver_url: batch.driver_url,
            item_code: batch.item_code,
            item_name: batch.item_name,
            warehouse: batch.warehouse,
            printer: batch.printer,
            print_mode: batch.print_mode,
            quantity_source: batch.quantity_source,
            manual_qty_kg: batch.manual_qty_kg,
            tare_enabled: batch.tare_enabled,
            tare_kg: batch.tare_kg,
            created_at: batch.created_at,
            updated_at: batch.updated_at,
            ..RpsBatchSession::default()
        }
    }
}

impl RpsBatchLmdbStore {
    pub fn open(path: PathBuf, map_size_bytes: usize) -> Result<Self, AppError> {
        std::fs::create_dir_all(&path)?;
        let env = unsafe {
            // This LMDB directory is owned by the RS-side RPS batch state store.
            EnvOpenOptions::new()
                .map_size(map_size_bytes.max(1024 * 1024))
                .max_dbs(1)
                .open(&path)
        }
        .map_err(lmdb_app_error)?;
        let mut wtxn = env.write_txn().map_err(lmdb_app_error)?;
        let db = env
            .create_database(&mut wtxn, Some("rps_batches"))
            .map_err(lmdb_app_error)?;
        wtxn.commit().map_err(lmdb_app_error)?;

        Ok(Self {
            env,
            db,
            write_lock: Arc::new(Mutex::new(())),
        })
    }
}

#[async_trait]
impl RpsBatchStorePort for RpsBatchLmdbStore {
    async fn get(&self, owner_key: &str) -> Result<Option<RpsBatchSession>, RpsBatchStoreError> {
        let rtxn = self.env.read_txn().map_err(lmdb_store_error)?;
        self.db
            .get(&rtxn, owner_key.trim().as_bytes())
            .map_err(lmdb_store_error)
    }

    async fn put(&self, batch: RpsBatchSession) -> Result<(), RpsBatchStoreError> {
        let _guard = self.write_lock.lock().await;
        let mut wtxn = self.env.write_txn().map_err(lmdb_store_error)?;
        self.db
            .put(&mut wtxn, batch.owner_key.trim().as_bytes(), &batch)
            .map_err(lmdb_store_error)?;
        wtxn.commit().map_err(lmdb_store_error)
    }
}

fn lmdb_app_error(error: heed::Error) -> AppError {
    AppError::Storage(format!("lmdb rps batch store failed: {error}"))
}

fn lmdb_store_error(_: heed::Error) -> RpsBatchStoreError {
    RpsBatchStoreError::StoreFailed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lmdb_batch_store_round_trips_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = RpsBatchLmdbStore::open(dir.path().join("batch.lmdb"), 1024 * 1024)
            .expect("lmdb store");
        let batch = RpsBatchSession {
            id: "batch-1".to_string(),
            active: true,
            owner_key: "werka:W-1".to_string(),
            item_code: "ITEM-1".to_string(),
            warehouse: "Stores - A".to_string(),
            ..RpsBatchSession::default()
        };

        store.put(batch.clone()).await.expect("put");

        assert_eq!(store.get("werka:W-1").await.expect("get"), Some(batch));
    }

    #[test]
    fn lmdb_batch_store_reads_pre_print_list_session() {
        let legacy = RpsBatchSessionV2 {
            id: "batch-v2".to_string(),
            owner_key: "material_taminotchi:M-1".to_string(),
            item_code: "ITEM-1".to_string(),
            warehouse: "Stores - A".to_string(),
            ..RpsBatchSessionV2::default()
        };
        let mut bytes = RPS_BATCH_MAGIC.to_vec();
        bytes.extend_from_slice(&bincode::serialize(&legacy).expect("serialize legacy batch"));

        let decoded = RpsBatchSessionCodec::bytes_decode(&bytes).expect("decode legacy batch");

        assert_eq!(decoded.id, "batch-v2");
        assert!(decoded.prints.is_empty());
    }
}
