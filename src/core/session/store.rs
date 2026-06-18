use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use heed::types::{Bytes, Unit};
use heed::{BoxedError, BytesDecode, BytesEncode, Database, Env, EnvOpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::sync::Mutex;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::models::SessionRecord;
use crate::error::AppError;
use crate::store::json_file;

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn get(&self, token: &str) -> Result<Option<SessionRecord>, AppError>;
    async fn put(&self, token: &str, record: SessionRecord) -> Result<(), AppError>;
    async fn delete(&self, token: &str) -> Result<(), AppError>;
}

#[derive(Clone)]
pub struct JsonSessionStore {
    path: Option<PathBuf>,
    state: Arc<Mutex<JsonSessionState>>,
}

#[derive(Default)]
struct JsonSessionState {
    loaded: bool,
    sessions: BTreeMap<String, SessionRecord>,
}

impl JsonSessionStore {
    pub fn persistent(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            state: Arc::new(Mutex::new(JsonSessionState::default())),
        }
    }

    pub fn memory() -> Self {
        Self {
            path: None,
            state: Arc::new(Mutex::new(JsonSessionState {
                loaded: true,
                sessions: BTreeMap::new(),
            })),
        }
    }

    async fn load_if_needed(&self, state: &mut JsonSessionState) -> Result<(), AppError> {
        if state.loaded {
            return Ok(());
        }
        state.sessions = match &self.path {
            Some(path) => json_file::read_map(path).await?,
            None => BTreeMap::new(),
        };
        let now = time::OffsetDateTime::now_utc();
        state.sessions.retain(|_, record| !record.is_expired(now));
        state.loaded = true;
        Ok(())
    }

    async fn save(&self, state: &JsonSessionState) -> Result<(), AppError> {
        if let Some(path) = &self.path {
            json_file::write_pretty(path, &state.sessions).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl SessionStore for JsonSessionStore {
    async fn get(&self, token: &str) -> Result<Option<SessionRecord>, AppError> {
        let mut state = self.state.lock().await;
        self.load_if_needed(&mut state).await?;
        Ok(state.sessions.get(token).cloned())
    }

    async fn put(&self, token: &str, record: SessionRecord) -> Result<(), AppError> {
        let mut state = self.state.lock().await;
        self.load_if_needed(&mut state).await?;
        state.sessions.insert(token.to_string(), record);
        self.save(&state).await
    }

    async fn delete(&self, token: &str) -> Result<(), AppError> {
        let mut state = self.state.lock().await;
        self.load_if_needed(&mut state).await?;
        if state.sessions.remove(token).is_some() {
            self.save(&state).await?;
        }
        Ok(())
    }
}

pub struct LmdbSessionStore {
    env: Env,
    db: Database<Bytes, SessionRecordCodec>,
    expires_db: Database<ExpiryKeyCodec, Unit>,
    write_lock: Arc<Mutex<()>>,
}

struct SessionRecordCodec;

const SESSION_RECORD_MAGIC: &[u8] = b"AMS2";

impl<'a> BytesEncode<'a> for SessionRecordCodec {
    type EItem = SessionRecord;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let payload = bincode::serialize(&StoredSessionRecord::from_record(item))?;
        let mut bytes = Vec::with_capacity(SESSION_RECORD_MAGIC.len() + payload.len());
        bytes.extend_from_slice(SESSION_RECORD_MAGIC);
        bytes.extend_from_slice(&payload);
        Ok(Cow::Owned(bytes))
    }
}

impl<'a> BytesDecode<'a> for SessionRecordCodec {
    type DItem = SessionRecord;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if let Some(payload) = bytes.strip_prefix(SESSION_RECORD_MAGIC) {
            let stored: StoredSessionRecord = bincode::deserialize(payload)?;
            return stored.into_record();
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[derive(Serialize, Deserialize)]
struct StoredSessionRecord {
    principal: StoredPrincipal,
    created_at_nanos: Option<i128>,
    updated_at_nanos: Option<i128>,
    expires_at_nanos: Option<i128>,
}

impl StoredSessionRecord {
    fn from_record(record: &SessionRecord) -> Self {
        Self {
            principal: StoredPrincipal::from_principal(&record.principal),
            created_at_nanos: record.created_at.map(OffsetDateTime::unix_timestamp_nanos),
            updated_at_nanos: record.updated_at.map(OffsetDateTime::unix_timestamp_nanos),
            expires_at_nanos: record.expires_at.map(OffsetDateTime::unix_timestamp_nanos),
        }
    }

    fn into_record(self) -> Result<SessionRecord, BoxedError> {
        Ok(SessionRecord {
            principal: self.principal.into_principal()?,
            created_at: decode_timestamp(self.created_at_nanos)?,
            updated_at: decode_timestamp(self.updated_at_nanos)?,
            expires_at: decode_timestamp(self.expires_at_nanos)?,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct StoredPrincipal {
    role: u8,
    display_name: String,
    legal_name: String,
    ref_: String,
    phone: String,
    avatar_url: String,
}

impl StoredPrincipal {
    fn from_principal(principal: &Principal) -> Self {
        Self {
            role: encode_role(&principal.role),
            display_name: principal.display_name.clone(),
            legal_name: principal.legal_name.clone(),
            ref_: principal.ref_.clone(),
            phone: principal.phone.clone(),
            avatar_url: principal.avatar_url.clone(),
        }
    }

    fn into_principal(self) -> Result<Principal, BoxedError> {
        Ok(Principal {
            role: decode_role(self.role)?,
            display_name: self.display_name,
            legal_name: self.legal_name,
            ref_: self.ref_,
            phone: self.phone,
            avatar_url: self.avatar_url,
        })
    }
}

fn encode_role(role: &PrincipalRole) -> u8 {
    match role {
        PrincipalRole::Supplier => 0,
        PrincipalRole::Werka => 1,
        PrincipalRole::Customer => 2,
        PrincipalRole::Admin => 3,
        PrincipalRole::Aparatchi => 4,
        PrincipalRole::Qolipchi => 5,
    }
}

fn decode_role(role: u8) -> Result<PrincipalRole, BoxedError> {
    match role {
        0 => Ok(PrincipalRole::Supplier),
        1 => Ok(PrincipalRole::Werka),
        2 => Ok(PrincipalRole::Customer),
        3 => Ok(PrincipalRole::Admin),
        4 => Ok(PrincipalRole::Aparatchi),
        5 => Ok(PrincipalRole::Qolipchi),
        _ => Err("invalid stored session principal role".into()),
    }
}

fn decode_timestamp(timestamp: Option<i128>) -> Result<Option<OffsetDateTime>, BoxedError> {
    timestamp
        .map(OffsetDateTime::from_unix_timestamp_nanos)
        .transpose()
        .map_err(Into::into)
}

impl LmdbSessionStore {
    pub fn open(path: PathBuf, map_size_bytes: usize) -> Result<Self, AppError> {
        std::fs::create_dir_all(&path)?;
        let map_size = map_size_bytes.max(1024 * 1024);
        let env = unsafe {
            // LMDB requires the caller to ensure the environment path is used
            // consistently. This service owns this directory for sessions only.
            EnvOpenOptions::new()
                .map_size(map_size)
                .max_dbs(2)
                .open(&path)
        }
        .map_err(lmdb_error)?;
        let mut wtxn = env.write_txn().map_err(lmdb_error)?;
        let db = env
            .create_database(&mut wtxn, Some("sessions"))
            .map_err(lmdb_error)?;
        let expires_db = env
            .create_database(&mut wtxn, Some("session_expiry"))
            .map_err(lmdb_error)?;
        wtxn.commit().map_err(lmdb_error)?;

        Ok(Self {
            env,
            db,
            expires_db,
            write_lock: Arc::new(Mutex::new(())),
        })
    }
}

#[async_trait]
impl SessionStore for LmdbSessionStore {
    async fn get(&self, token: &str) -> Result<Option<SessionRecord>, AppError> {
        let key = session_key(token);
        let record = {
            let rtxn = self.env.read_txn().map_err(lmdb_error)?;
            self.db.get(&rtxn, &key).map_err(lmdb_error)?
        };
        if record.is_some() {
            return Ok(record);
        }

        let legacy_record = {
            let rtxn = self.env.read_txn().map_err(lmdb_error)?;
            self.db.get(&rtxn, token.as_bytes()).map_err(lmdb_error)?
        };
        if let Some(record) = legacy_record {
            self.put(token, record.clone()).await?;
            self.delete_legacy_key(token).await?;
            return Ok(Some(record));
        }

        Ok(None)
    }

    async fn put(&self, token: &str, record: SessionRecord) -> Result<(), AppError> {
        let key = session_key(token);
        let _guard = self.write_lock.lock().await;
        let mut wtxn = self.env.write_txn().map_err(lmdb_error)?;
        self.purge_expired_in_txn(&mut wtxn, OffsetDateTime::now_utc())?;
        if let Some(previous) = self.db.get(&wtxn, &key).map_err(lmdb_error)? {
            self.delete_expiry_index(&mut wtxn, &key, &previous)?;
        }
        self.db.put(&mut wtxn, &key, &record).map_err(lmdb_error)?;
        self.put_expiry_index(&mut wtxn, &key, &record)?;
        wtxn.commit().map_err(lmdb_error)
    }

    async fn delete(&self, token: &str) -> Result<(), AppError> {
        let key = session_key(token);
        let _guard = self.write_lock.lock().await;
        let mut wtxn = self.env.write_txn().map_err(lmdb_error)?;
        if let Some(previous) = self.db.get(&wtxn, &key).map_err(lmdb_error)? {
            self.delete_expiry_index(&mut wtxn, &key, &previous)?;
        }
        self.db.delete(&mut wtxn, &key).map_err(lmdb_error)?;
        self.db
            .delete(&mut wtxn, token.as_bytes())
            .map_err(lmdb_error)?;
        wtxn.commit().map_err(lmdb_error)
    }
}

impl LmdbSessionStore {
    async fn delete_legacy_key(&self, token: &str) -> Result<(), AppError> {
        let _guard = self.write_lock.lock().await;
        let mut wtxn = self.env.write_txn().map_err(lmdb_error)?;
        self.db
            .delete(&mut wtxn, token.as_bytes())
            .map_err(lmdb_error)?;
        wtxn.commit().map_err(lmdb_error)
    }

    fn put_expiry_index(
        &self,
        wtxn: &mut heed::RwTxn<'_>,
        session_key: &[u8; 32],
        record: &SessionRecord,
    ) -> Result<(), AppError> {
        if let Some(expires_at) = record.expires_at {
            let key = ExpiryKey::new(expires_at, *session_key);
            self.expires_db.put(wtxn, &key, &()).map_err(lmdb_error)?;
        }
        Ok(())
    }

    fn delete_expiry_index(
        &self,
        wtxn: &mut heed::RwTxn<'_>,
        session_key: &[u8; 32],
        record: &SessionRecord,
    ) -> Result<(), AppError> {
        if let Some(expires_at) = record.expires_at {
            let key = ExpiryKey::new(expires_at, *session_key);
            self.expires_db.delete(wtxn, &key).map_err(lmdb_error)?;
        }
        Ok(())
    }

    fn purge_expired_in_txn(
        &self,
        wtxn: &mut heed::RwTxn<'_>,
        now: OffsetDateTime,
    ) -> Result<usize, AppError> {
        let upper = ExpiryKey::new(now, [u8::MAX; 32]);
        let expired = {
            let mut iter = self
                .expires_db
                .range(&*wtxn, &(..=upper))
                .map_err(lmdb_error)?;
            let mut keys = Vec::new();
            while let Some((key, ())) = iter.next().transpose().map_err(lmdb_error)? {
                keys.push(key);
            }
            keys
        };

        for key in &expired {
            self.db.delete(wtxn, &key.session_key).map_err(lmdb_error)?;
            self.expires_db.delete(wtxn, key).map_err(lmdb_error)?;
        }

        Ok(expired.len())
    }
}

fn session_key(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExpiryKey {
    expires_at_nanos: i128,
    session_key: [u8; 32],
}

impl ExpiryKey {
    fn new(expires_at: OffsetDateTime, session_key: [u8; 32]) -> Self {
        Self {
            expires_at_nanos: expires_at.unix_timestamp_nanos(),
            session_key,
        }
    }
}

struct ExpiryKeyCodec;

impl<'a> BytesEncode<'a> for ExpiryKeyCodec {
    type EItem = ExpiryKey;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let mut bytes = Vec::with_capacity(48);
        bytes.extend_from_slice(&encode_ordered_i128(item.expires_at_nanos));
        bytes.extend_from_slice(&item.session_key);
        Ok(Cow::Owned(bytes))
    }
}

impl<'a> BytesDecode<'a> for ExpiryKeyCodec {
    type DItem = ExpiryKey;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        let expires_at = bytes
            .get(..16)
            .and_then(|bytes| bytes.try_into().ok())
            .map(decode_ordered_i128)
            .ok_or("invalid expiry key timestamp")?;
        let session_key = bytes
            .get(16..48)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or("invalid expiry key session hash")?;
        Ok(ExpiryKey {
            expires_at_nanos: expires_at,
            session_key,
        })
    }
}

fn encode_ordered_i128(value: i128) -> [u8; 16] {
    ((value as u128) ^ (1_u128 << 127)).to_be_bytes()
}

fn decode_ordered_i128(bytes: [u8; 16]) -> i128 {
    (u128::from_be_bytes(bytes) ^ (1_u128 << 127)) as i128
}

fn lmdb_error(error: heed::Error) -> AppError {
    AppError::Storage(format!("lmdb session store failed: {error}"))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
