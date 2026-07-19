use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use heed::types::{Bytes, Unit};
use heed::{Database, Env, EnvOpenOptions};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::sync::Mutex;

use super::SessionStore;
use super::lmdb_codec::SessionRecordCodec;
use super::lmdb_expiry::{ExpiryKey, ExpiryKeyCodec};
use crate::core::auth::models::PrincipalRole;
use crate::core::session::models::SessionRecord;
use crate::error::AppError;

pub struct LmdbSessionStore {
    pub(super) env: Env,
    pub(super) db: Database<Bytes, SessionRecordCodec>,
    pub(super) expires_db: Database<ExpiryKeyCodec, Unit>,
    write_lock: Arc<Mutex<()>>,
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

    async fn delete_for_principal(
        &self,
        role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<usize, AppError> {
        let principal_ref = principal_ref.trim();
        if principal_ref.is_empty() {
            return Ok(0);
        }
        let _guard = self.write_lock.lock().await;
        let mut wtxn = self.env.write_txn().map_err(lmdb_error)?;
        let matches = {
            let mut iter = self.db.iter(&wtxn).map_err(lmdb_error)?;
            let mut matches = Vec::new();
            while let Some((key, record)) = iter.next().transpose().map_err(lmdb_error)? {
                if record.principal.role == *role && record.principal.ref_.trim() == principal_ref {
                    matches.push((key.to_vec(), record));
                }
            }
            matches
        };
        for (key, record) in &matches {
            if let Ok(hashed_key) = <&[u8; 32]>::try_from(key.as_slice()) {
                self.delete_expiry_index(&mut wtxn, hashed_key, record)?;
            }
            self.db.delete(&mut wtxn, key).map_err(lmdb_error)?;
        }
        wtxn.commit().map_err(lmdb_error)?;
        Ok(matches.len())
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

pub(super) fn session_key(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn lmdb_error(error: heed::Error) -> AppError {
    AppError::Storage(format!("lmdb session store failed: {error}"))
}
