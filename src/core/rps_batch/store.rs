use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use heed::types::Bytes;
use heed::{BoxedError, BytesDecode, BytesEncode, Database, Env, EnvOpenOptions};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::models::{RpsBatchSession, is_valid_batch_code};
use super::ports::{RpsBatchStoreError, RpsBatchStorePort};
use crate::error::AppError;

pub struct RpsBatchLmdbStore {
    env: Env,
    db: Database<Bytes, RpsBatchSessionCodec>,
    write_lock: Arc<Mutex<()>>,
}

struct RpsBatchSessionCodec;

const RPS_BATCH_MAGIC: &[u8] = b"RPSB1";
const RPS_BATCH_JSON_MAGIC: &[u8] = b"RPSJ1";

impl<'a> BytesEncode<'a> for RpsBatchSessionCodec {
    type EItem = RpsBatchSession;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let payload = serde_json::to_vec(item)?;
        let mut bytes = Vec::with_capacity(RPS_BATCH_JSON_MAGIC.len() + payload.len());
        bytes.extend_from_slice(RPS_BATCH_JSON_MAGIC);
        bytes.extend_from_slice(&payload);
        Ok(Cow::Owned(bytes))
    }
}

impl<'a> BytesDecode<'a> for RpsBatchSessionCodec {
    type DItem = RpsBatchSession;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if let Some(payload) = bytes.strip_prefix(RPS_BATCH_JSON_MAGIC) {
            let mut batch: RpsBatchSession = serde_json::from_slice(payload)?;
            batch.ensure_context();
            return Ok(batch);
        }
        if let Some(payload) = bytes.strip_prefix(RPS_BATCH_MAGIC) {
            let mut batch: RpsBatchSession = match bincode::deserialize::<RpsBatchSessionV5>(payload) {
                Ok(batch) => serde_json::from_value(serde_json::to_value(batch)?)?,
                Err(_) => match bincode::deserialize::<RpsBatchSessionV4>(payload) {
                    Ok(batch) => batch.into(),
                    Err(_) => match bincode::deserialize::<RpsBatchSessionV3>(payload) {
                        Ok(batch) => batch.into(),
                        Err(_) => match bincode::deserialize::<RpsBatchSessionV2>(payload) {
                            Ok(batch) => batch.into(),
                            Err(_) => bincode::deserialize::<RpsBatchSessionV1>(payload)?.into(),
                        },
                    },
                },
            };
            batch.ensure_context();
            return Ok(batch);
        }
        let mut batch: RpsBatchSession = serde_json::from_slice(bytes)?;
        batch.ensure_context();
        Ok(batch)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct RpsBatchSessionV5 {
    id: String,
    batch_code: String,
    revision: u64,
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
    width_mm: Option<f64>,
    micron: Option<f64>,
    length_m: Option<f64>,
    last_error: String,
    last_error_at: String,
    prints: Vec<super::models::RpsBatchPrintEntry>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct RpsBatchSessionV4 {
    id: String,
    batch_code: String,
    revision: u64,
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
    width_mm: Option<f64>,
    micron: Option<f64>,
    last_error: String,
    last_error_at: String,
    prints: Vec<super::models::RpsBatchPrintEntry>,
    created_at: String,
    updated_at: String,
}

impl From<RpsBatchSessionV4> for RpsBatchSession {
    fn from(batch: RpsBatchSessionV4) -> Self {
        Self {
            order_assignment: None,
            id: batch.id,
            batch_code: batch.batch_code,
            revision: batch.revision,
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
            width_mm: batch.width_mm,
            micron: batch.micron,
            length_m: None,
            last_error: batch.last_error,
            last_error_at: batch.last_error_at,
            prints: batch.prints,
            created_at: batch.created_at,
            updated_at: batch.updated_at,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct RpsBatchSessionV3 {
    id: String,
    batch_code: String,
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
    prints: Vec<super::models::RpsBatchPrintEntry>,
    created_at: String,
    updated_at: String,
}

impl From<RpsBatchSessionV3> for RpsBatchSession {
    fn from(batch: RpsBatchSessionV3) -> Self {
        Self {
            order_assignment: None,
            id: batch.id,
            batch_code: batch.batch_code,
            revision: 1,
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
            width_mm: None,
            micron: None,
            length_m: None,
            last_error: batch.last_error,
            last_error_at: batch.last_error_at,
            prints: batch.prints,
            created_at: batch.created_at,
            updated_at: batch.updated_at,
        }
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

    async fn put(&self, batch: &RpsBatchSession) -> Result<(), RpsBatchStoreError> {
        let _guard = self.write_lock.lock().await;
        let batch = batch.contextualized();
        self.ensure_unique_batch_code(batch.as_ref())?;
        let mut wtxn = self.env.write_txn().map_err(lmdb_store_error)?;
        self.db
            .put(&mut wtxn, batch.owner_key.trim().as_bytes(), batch.as_ref())
            .map_err(lmdb_store_error)?;
        wtxn.commit().map_err(lmdb_store_error)
    }

    async fn complete(&self, batch: &RpsBatchSession) -> Result<(), RpsBatchStoreError> {
        let _guard = self.write_lock.lock().await;
        let batch = batch.contextualized();
        self.ensure_unique_batch_code(batch.as_ref())?;
        let mut wtxn = self.env.write_txn().map_err(lmdb_store_error)?;
        self.db
            .put(&mut wtxn, batch.owner_key.trim().as_bytes(), batch.as_ref())
            .map_err(lmdb_store_error)?;
        self.db
            .put(
                &mut wtxn,
                history_key(batch.as_ref()).as_bytes(),
                batch.as_ref(),
            )
            .map_err(lmdb_store_error)?;
        wtxn.commit().map_err(lmdb_store_error)
    }

    async fn list_completed(
        &self,
        owner_key: &str,
        limit: usize,
    ) -> Result<Vec<RpsBatchSession>, RpsBatchStoreError> {
        let prefix = format!("history:{}:", owner_key.trim());
        let rtxn = self.env.read_txn().map_err(lmdb_store_error)?;
        let mut batches = self
            .db
            .iter(&rtxn)
            .map_err(lmdb_store_error)?
            .filter_map(|entry| match entry {
                Ok((key, batch)) if key.starts_with(prefix.as_bytes()) => Some(Ok(batch)),
                Ok(_) => None,
                Err(error) => Some(Err(lmdb_store_error(error))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        batches.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        batches.truncate(limit.min(100));
        Ok(batches)
    }
}

impl RpsBatchLmdbStore {
    fn ensure_unique_batch_code(&self, batch: &RpsBatchSession) -> Result<(), RpsBatchStoreError> {
        if !is_valid_batch_code(&batch.batch_code) {
            return Err(RpsBatchStoreError::StoreFailed);
        }
        let rtxn = self.env.read_txn().map_err(lmdb_store_error)?;
        for entry in self.db.iter(&rtxn).map_err(lmdb_store_error)? {
            let (_, stored) = entry.map_err(lmdb_store_error)?;
            if stored.batch_code == batch.batch_code
                && (stored.owner_key != batch.owner_key || stored.id != batch.id)
            {
                return Err(RpsBatchStoreError::StoreFailed);
            }
        }
        Ok(())
    }
}

fn history_key(batch: &RpsBatchSession) -> String {
    format!("history:{}:{}", batch.owner_key.trim(), batch.id.trim())
}

fn lmdb_app_error(error: heed::Error) -> AppError {
    AppError::Storage(format!("lmdb rps batch store failed: {error}"))
}

fn lmdb_store_error(_: heed::Error) -> RpsBatchStoreError {
    RpsBatchStoreError::StoreFailed
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
