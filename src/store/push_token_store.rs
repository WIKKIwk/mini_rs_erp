use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use heed::types::{Bytes, Str};
use heed::{BoxedError, BytesDecode, BytesEncode, Database, Env, EnvOpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::sync::Mutex;

use crate::core::push::models::PushTokenRecord;
use crate::core::push::ports::{PushStoreError, PushTokenStorePort};
use crate::error::AppError;
use crate::store::json_file::{read_map, write_pretty};

#[derive(Clone)]
pub struct PushTokenStore {
    path: PathBuf,
    state: Arc<Mutex<PushTokenStoreState>>,
}

#[derive(Default)]
struct PushTokenStoreState {
    loaded: bool,
    cache: HashMap<String, Vec<PushTokenRecord>>,
}

impl PushTokenStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: Arc::new(Mutex::new(PushTokenStoreState::default())),
        }
    }
}

#[async_trait]
impl PushTokenStorePort for PushTokenStore {
    async fn move_token_to_key(
        &self,
        target_key: &str,
        token: &str,
        platform: &str,
    ) -> Result<(), PushStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        let token = token.trim();
        for records in state.cache.values_mut() {
            records.retain(|record| record.token.trim() != token);
        }
        state.cache.retain(|_, records| !records.is_empty());

        let key = target_key.trim().to_string();
        let records = state.cache.entry(key).or_default();
        records.retain(|record| record.token.trim() != token);
        records.push(PushTokenRecord {
            token: token.to_string(),
            platform: platform.trim().to_string(),
            updated_at: OffsetDateTime::now_utc(),
        });
        write_pretty(&self.path, &state.cache)
            .await
            .map_err(|_| PushStoreError::StoreFailed)
    }

    async fn delete(&self, key: &str, token: &str) -> Result<(), PushStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        let key = key.trim();
        let token = token.trim();
        if let Some(records) = state.cache.get_mut(key) {
            records.retain(|record| record.token.trim() != token);
            if records.is_empty() {
                state.cache.remove(key);
            }
        }
        write_pretty(&self.path, &state.cache)
            .await
            .map_err(|_| PushStoreError::StoreFailed)
    }

    async fn list(&self, key: &str) -> Result<Vec<PushTokenRecord>, PushStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        Ok(state.cache.get(key.trim()).cloned().unwrap_or_default())
    }
}

async fn load_if_needed(
    path: &Path,
    state: &mut PushTokenStoreState,
) -> Result<(), PushStoreError> {
    if state.loaded {
        return Ok(());
    }
    let data = read_map::<Vec<PushTokenRecord>>(path)
        .await
        .map_err(|_| PushStoreError::StoreFailed)?;
    state.cache = data.into_iter().collect();
    state.loaded = true;
    Ok(())
}

pub struct LmdbPushTokenStore {
    env: Env,
    records_db: Database<Str, PushTokenRecordsCodec>,
    owner_db: Database<Bytes, Str>,
    legacy_json_path: Option<PathBuf>,
    legacy_migrated: Arc<AtomicBool>,
    legacy_migration_lock: Arc<Mutex<()>>,
    write_lock: Arc<Mutex<()>>,
}

struct PushTokenRecordsCodec;

const PUSH_TOKEN_RECORDS_MAGIC: &[u8] = b"AMT1";

impl<'a> BytesEncode<'a> for PushTokenRecordsCodec {
    type EItem = Vec<PushTokenRecord>;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let records: Vec<StoredPushTokenRecord> = item
            .iter()
            .map(StoredPushTokenRecord::from_record)
            .collect();
        let payload = bincode::serialize(&records)?;
        let mut bytes = Vec::with_capacity(PUSH_TOKEN_RECORDS_MAGIC.len() + payload.len());
        bytes.extend_from_slice(PUSH_TOKEN_RECORDS_MAGIC);
        bytes.extend_from_slice(&payload);
        Ok(Cow::Owned(bytes))
    }
}

impl<'a> BytesDecode<'a> for PushTokenRecordsCodec {
    type DItem = Vec<PushTokenRecord>;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if let Some(payload) = bytes.strip_prefix(PUSH_TOKEN_RECORDS_MAGIC) {
            let records: Vec<StoredPushTokenRecord> = bincode::deserialize(payload)?;
            return records
                .into_iter()
                .map(StoredPushTokenRecord::into_record)
                .collect();
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[derive(Serialize, Deserialize)]
struct StoredPushTokenRecord {
    token: String,
    platform: String,
    updated_at_nanos: i128,
}

impl StoredPushTokenRecord {
    fn from_record(record: &PushTokenRecord) -> Self {
        Self {
            token: record.token.clone(),
            platform: record.platform.clone(),
            updated_at_nanos: record.updated_at.unix_timestamp_nanos(),
        }
    }

    fn into_record(self) -> Result<PushTokenRecord, BoxedError> {
        Ok(PushTokenRecord {
            token: self.token,
            platform: self.platform,
            updated_at: OffsetDateTime::from_unix_timestamp_nanos(self.updated_at_nanos)?,
        })
    }
}

impl LmdbPushTokenStore {
    pub fn open(
        path: PathBuf,
        map_size_bytes: usize,
        legacy_json_path: Option<PathBuf>,
    ) -> Result<Self, AppError> {
        std::fs::create_dir_all(&path)?;
        let map_size = map_size_bytes.max(1024 * 1024);
        let env = unsafe {
            // This LMDB directory is owned by the push token store.
            EnvOpenOptions::new()
                .map_size(map_size)
                .max_dbs(2)
                .open(&path)
        }
        .map_err(lmdb_app_error)?;
        let mut wtxn = env.write_txn().map_err(lmdb_app_error)?;
        let records_db = env
            .create_database(&mut wtxn, Some("push_tokens"))
            .map_err(lmdb_app_error)?;
        let owner_db = env
            .create_database(&mut wtxn, Some("push_token_owner"))
            .map_err(lmdb_app_error)?;
        wtxn.commit().map_err(lmdb_app_error)?;

        Ok(Self {
            env,
            records_db,
            owner_db,
            legacy_json_path,
            legacy_migrated: Arc::new(AtomicBool::new(false)),
            legacy_migration_lock: Arc::new(Mutex::new(())),
            write_lock: Arc::new(Mutex::new(())),
        })
    }

    async fn migrate_legacy_if_needed(&self) -> Result<(), PushStoreError> {
        if self.legacy_migrated.load(Ordering::Acquire) {
            return Ok(());
        }

        let _migration_guard = self.legacy_migration_lock.lock().await;
        if self.legacy_migrated.load(Ordering::Acquire) {
            return Ok(());
        }

        let Some(path) = &self.legacy_json_path else {
            self.legacy_migrated.store(true, Ordering::Release);
            return Ok(());
        };

        let data = read_map::<Vec<PushTokenRecord>>(path)
            .await
            .map_err(|_| PushStoreError::StoreFailed)?;
        if data.is_empty() {
            self.legacy_migrated.store(true, Ordering::Release);
            return Ok(());
        }

        let _guard = self.write_lock.lock().await;
        let mut wtxn = self.env.write_txn().map_err(lmdb_store_error)?;
        for (key, records) in data {
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            for record in records {
                let record = normalize_record(record);
                if record.token.is_empty() {
                    continue;
                }
                self.move_record_to_key_in_txn(&mut wtxn, key, record)?;
            }
        }
        wtxn.commit().map_err(lmdb_store_error)?;
        self.legacy_migrated.store(true, Ordering::Release);
        Ok(())
    }

    fn move_record_to_key_in_txn(
        &self,
        wtxn: &mut heed::RwTxn<'_>,
        target_key: &str,
        record: PushTokenRecord,
    ) -> Result<(), PushStoreError> {
        let token = record.token.trim();
        let token_key = push_token_hash(token);
        let previous_key = self
            .owner_db
            .get(&*wtxn, &token_key)
            .map_err(lmdb_store_error)?
            .map(str::to_string);
        if let Some(previous_key) = previous_key.as_deref()
            && previous_key != target_key
        {
            self.remove_token_from_key_in_txn(wtxn, previous_key, token)?;
        }

        let mut records = self
            .records_db
            .get(&*wtxn, target_key)
            .map_err(lmdb_store_error)?
            .unwrap_or_default();
        records.retain(|stored| stored.token.trim() != token);
        records.push(record);
        self.records_db
            .put(wtxn, target_key, &records)
            .map_err(lmdb_store_error)?;
        self.owner_db
            .put(wtxn, &token_key, target_key)
            .map_err(lmdb_store_error)
    }

    fn remove_token_from_key_in_txn(
        &self,
        wtxn: &mut heed::RwTxn<'_>,
        key: &str,
        token: &str,
    ) -> Result<bool, PushStoreError> {
        let Some(mut records) = self.records_db.get(&*wtxn, key).map_err(lmdb_store_error)? else {
            return Ok(false);
        };
        let before = records.len();
        records.retain(|record| record.token.trim() != token);
        if records.is_empty() {
            self.records_db
                .delete(wtxn, key)
                .map_err(lmdb_store_error)?;
        } else if records.len() != before {
            self.records_db
                .put(wtxn, key, &records)
                .map_err(lmdb_store_error)?;
        }
        Ok(records.len() != before)
    }
}

#[async_trait]
impl PushTokenStorePort for LmdbPushTokenStore {
    async fn move_token_to_key(
        &self,
        target_key: &str,
        token: &str,
        platform: &str,
    ) -> Result<(), PushStoreError> {
        self.migrate_legacy_if_needed().await?;
        let token = token.trim();
        let target_key = target_key.trim();
        let record = PushTokenRecord {
            token: token.to_string(),
            platform: platform.trim().to_string(),
            updated_at: OffsetDateTime::now_utc(),
        };

        let _guard = self.write_lock.lock().await;
        if self.token_already_registered(target_key, token, &record.platform)? {
            return Ok(());
        }
        let mut wtxn = self.env.write_txn().map_err(lmdb_store_error)?;
        self.move_record_to_key_in_txn(&mut wtxn, target_key, record)?;
        wtxn.commit().map_err(lmdb_store_error)
    }

    async fn delete(&self, key: &str, token: &str) -> Result<(), PushStoreError> {
        self.migrate_legacy_if_needed().await?;
        let key = key.trim();
        let token = token.trim();
        let token_key = push_token_hash(token);

        let _guard = self.write_lock.lock().await;
        let mut wtxn = self.env.write_txn().map_err(lmdb_store_error)?;
        self.remove_token_from_key_in_txn(&mut wtxn, key, token)?;
        if self
            .owner_db
            .get(&wtxn, &token_key)
            .map_err(lmdb_store_error)?
            .is_some_and(|owner| owner == key)
        {
            self.owner_db
                .delete(&mut wtxn, &token_key)
                .map_err(lmdb_store_error)?;
        }
        wtxn.commit().map_err(lmdb_store_error)
    }

    async fn list(&self, key: &str) -> Result<Vec<PushTokenRecord>, PushStoreError> {
        self.migrate_legacy_if_needed().await?;
        let rtxn = self.env.read_txn().map_err(lmdb_store_error)?;
        Ok(self
            .records_db
            .get(&rtxn, key.trim())
            .map_err(lmdb_store_error)?
            .unwrap_or_default())
    }
}

impl LmdbPushTokenStore {
    fn token_already_registered(
        &self,
        target_key: &str,
        token: &str,
        platform: &str,
    ) -> Result<bool, PushStoreError> {
        let token_key = push_token_hash(token);
        let rtxn = self.env.read_txn().map_err(lmdb_store_error)?;
        if self
            .owner_db
            .get(&rtxn, &token_key)
            .map_err(lmdb_store_error)?
            .is_none_or(|owner| owner != target_key)
        {
            return Ok(false);
        }

        Ok(self
            .records_db
            .get(&rtxn, target_key)
            .map_err(lmdb_store_error)?
            .unwrap_or_default()
            .iter()
            .any(|record| record.token.trim() == token && record.platform.trim() == platform))
    }
}

fn normalize_record(record: PushTokenRecord) -> PushTokenRecord {
    PushTokenRecord {
        token: record.token.trim().to_string(),
        platform: record.platform.trim().to_string(),
        updated_at: record.updated_at,
    }
}

fn push_token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn lmdb_app_error(error: heed::Error) -> AppError {
    AppError::Storage(format!("lmdb push token store failed: {error}"))
}

fn lmdb_store_error(_: heed::Error) -> PushStoreError {
    PushStoreError::StoreFailed
}
