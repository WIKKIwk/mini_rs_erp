use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use heed::types::Str;
use heed::{BoxedError, BytesDecode, BytesEncode, Database, Env, EnvOpenOptions};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::core::admin::models::AdminState;
use crate::core::admin::ports::{AdminPortError, AdminStatePort};
use crate::core::auth::ports::{AdminAccessState, AdminAccessStateLookup, AuthPortError};
use crate::core::werka::ports::{
    WerkaPortError, WerkaSupplierAdminState, WerkaSupplierAdminStateLookup,
};
use crate::error::AppError;
use crate::store::json_file;

#[derive(Debug, Clone)]
pub struct AdminSupplierStateStore {
    path: PathBuf,
}

impl AdminSupplierStateStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl AdminStatePort for AdminSupplierStateStore {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        let raw: BTreeMap<String, AdminSupplierStateRecord> = json_file::read_map(&self.path)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;

        Ok(records_to_admin_states(raw))
    }

    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
        let mut raw: BTreeMap<String, AdminSupplierStateRecord> = json_file::read_map(&self.path)
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        raw.insert(
            ref_.trim().to_string(),
            AdminSupplierStateRecord::from_admin_state(state),
        );
        json_file::write_pretty(&self.path, &raw)
            .await
            .map_err(|_| AdminPortError::LookupFailed)
    }
}

#[async_trait]
impl AdminAccessStateLookup for AdminSupplierStateStore {
    async fn list_states(&self) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError> {
        let raw: BTreeMap<String, AdminSupplierStateRecord> = json_file::read_map(&self.path)
            .await
            .map_err(|_| AuthPortError::LookupFailed)?;

        Ok(records_to_access_states(raw))
    }
}

#[async_trait]
impl WerkaSupplierAdminStateLookup for AdminSupplierStateStore {
    async fn werka_supplier_admin_state(
        &self,
        supplier_ref: &str,
    ) -> Result<WerkaSupplierAdminState, WerkaPortError> {
        let raw: BTreeMap<String, AdminSupplierStateRecord> = json_file::read_map(&self.path)
            .await
            .map_err(|_| WerkaPortError::LookupFailed)?;
        let Some(state) = raw.get(supplier_ref.trim()) else {
            return Ok(WerkaSupplierAdminState::default());
        };
        Ok(WerkaSupplierAdminState {
            blocked: state.blocked,
            removed: state.removed,
            assigned_item_codes: state.assigned_item_codes.clone(),
        })
    }
}

pub enum AdminSupplierStateBackend {
    Json(AdminSupplierStateStore),
    Lmdb(LmdbAdminSupplierStateStore),
}

impl AdminSupplierStateBackend {
    pub fn json(path: PathBuf) -> Self {
        Self::Json(AdminSupplierStateStore::new(path))
    }

    pub fn lmdb(
        path: PathBuf,
        map_size_bytes: usize,
        legacy_json_path: Option<PathBuf>,
    ) -> Result<Self, AppError> {
        Ok(Self::Lmdb(LmdbAdminSupplierStateStore::open(
            path,
            map_size_bytes,
            legacy_json_path,
        )?))
    }
}

#[async_trait]
impl AdminStatePort for AdminSupplierStateBackend {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        match self {
            Self::Json(store) => store.states().await,
            Self::Lmdb(store) => store.states().await,
        }
    }

    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
        match self {
            Self::Json(store) => store.put_state(ref_, state).await,
            Self::Lmdb(store) => store.put_state(ref_, state).await,
        }
    }
}

#[async_trait]
impl AdminAccessStateLookup for AdminSupplierStateBackend {
    async fn list_states(&self) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError> {
        match self {
            Self::Json(store) => store.list_states().await,
            Self::Lmdb(store) => store.list_states().await,
        }
    }
}

#[async_trait]
impl WerkaSupplierAdminStateLookup for AdminSupplierStateBackend {
    async fn werka_supplier_admin_state(
        &self,
        supplier_ref: &str,
    ) -> Result<WerkaSupplierAdminState, WerkaPortError> {
        match self {
            Self::Json(store) => store.werka_supplier_admin_state(supplier_ref).await,
            Self::Lmdb(store) => store.werka_supplier_admin_state(supplier_ref).await,
        }
    }
}

pub struct LmdbAdminSupplierStateStore {
    env: Env,
    db: Database<Str, AdminSupplierStateRecordCodec>,
    legacy_json_path: Option<PathBuf>,
    legacy_migrated: Arc<Mutex<bool>>,
    write_lock: Arc<Mutex<()>>,
}

struct AdminSupplierStateRecordCodec;

const ADMIN_SUPPLIER_STATE_MAGIC: &[u8] = b"AMA1";

impl<'a> BytesEncode<'a> for AdminSupplierStateRecordCodec {
    type EItem = AdminSupplierStateRecord;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let payload = bincode::serialize(&StoredAdminSupplierStateRecord::from_record(item))?;
        let mut bytes = Vec::with_capacity(ADMIN_SUPPLIER_STATE_MAGIC.len() + payload.len());
        bytes.extend_from_slice(ADMIN_SUPPLIER_STATE_MAGIC);
        bytes.extend_from_slice(&payload);
        Ok(Cow::Owned(bytes))
    }
}

impl<'a> BytesDecode<'a> for AdminSupplierStateRecordCodec {
    type DItem = AdminSupplierStateRecord;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if let Some(payload) = bytes.strip_prefix(ADMIN_SUPPLIER_STATE_MAGIC) {
            let stored: StoredAdminSupplierStateRecord = bincode::deserialize(payload)?;
            return stored.into_record();
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[derive(Serialize, Deserialize)]
struct StoredAdminSupplierStateRecord {
    custom_code: String,
    blocked: bool,
    removed: bool,
    assignments_configured: bool,
    assigned_item_codes: Vec<String>,
    pending_persist_code: String,
    pending_persist_at_nanos: Option<i128>,
    regen_window_started_at_nanos: Option<i128>,
    regen_window_count: i32,
    cooldown_until_nanos: Option<i128>,
}

impl StoredAdminSupplierStateRecord {
    fn from_record(record: &AdminSupplierStateRecord) -> Self {
        Self {
            custom_code: record.custom_code.clone(),
            blocked: record.blocked,
            removed: record.removed,
            assignments_configured: record.assignments_configured,
            assigned_item_codes: record.assigned_item_codes.clone(),
            pending_persist_code: record.pending_persist_code.clone(),
            pending_persist_at_nanos: record
                .pending_persist_at
                .map(time::OffsetDateTime::unix_timestamp_nanos),
            regen_window_started_at_nanos: record
                .regen_window_started_at
                .map(time::OffsetDateTime::unix_timestamp_nanos),
            regen_window_count: record.regen_window_count,
            cooldown_until_nanos: record
                .cooldown_until
                .map(time::OffsetDateTime::unix_timestamp_nanos),
        }
    }

    fn into_record(self) -> Result<AdminSupplierStateRecord, BoxedError> {
        Ok(AdminSupplierStateRecord {
            custom_code: self.custom_code,
            blocked: self.blocked,
            removed: self.removed,
            assignments_configured: self.assignments_configured,
            assigned_item_codes: self.assigned_item_codes,
            pending_persist_code: self.pending_persist_code,
            pending_persist_at: decode_timestamp(self.pending_persist_at_nanos)?,
            regen_window_started_at: decode_timestamp(self.regen_window_started_at_nanos)?,
            regen_window_count: self.regen_window_count,
            cooldown_until: decode_timestamp(self.cooldown_until_nanos)?,
        })
    }
}

impl LmdbAdminSupplierStateStore {
    pub fn open(
        path: PathBuf,
        map_size_bytes: usize,
        legacy_json_path: Option<PathBuf>,
    ) -> Result<Self, AppError> {
        std::fs::create_dir_all(&path)?;
        let map_size = map_size_bytes.max(1024 * 1024);
        let env = unsafe {
            // This LMDB directory is owned by the admin supplier/customer state store.
            EnvOpenOptions::new()
                .map_size(map_size)
                .max_dbs(1)
                .open(&path)
        }
        .map_err(lmdb_app_error)?;
        let mut wtxn = env.write_txn().map_err(lmdb_app_error)?;
        let db = env
            .create_database(&mut wtxn, Some("admin_supplier_state"))
            .map_err(lmdb_app_error)?;
        wtxn.commit().map_err(lmdb_app_error)?;

        Ok(Self {
            env,
            db,
            legacy_json_path,
            legacy_migrated: Arc::new(Mutex::new(false)),
            write_lock: Arc::new(Mutex::new(())),
        })
    }

    async fn migrate_legacy_if_needed(&self) -> Result<(), AppError> {
        let mut migrated = self.legacy_migrated.lock().await;
        if *migrated {
            return Ok(());
        }
        let Some(path) = &self.legacy_json_path else {
            *migrated = true;
            return Ok(());
        };
        let data: BTreeMap<String, AdminSupplierStateRecord> = json_file::read_map(path).await?;
        if data.is_empty() {
            *migrated = true;
            return Ok(());
        }

        let _guard = self.write_lock.lock().await;
        let mut wtxn = self.env.write_txn().map_err(lmdb_app_error)?;
        for (key, record) in data {
            let key = key.trim();
            if !key.is_empty() && self.db.get(&wtxn, key).map_err(lmdb_app_error)?.is_none() {
                self.db
                    .put(&mut wtxn, key, &record)
                    .map_err(lmdb_app_error)?;
            }
        }
        wtxn.commit().map_err(lmdb_app_error)?;
        *migrated = true;
        Ok(())
    }

    async fn raw_states(&self) -> Result<BTreeMap<String, AdminSupplierStateRecord>, AppError> {
        self.migrate_legacy_if_needed().await?;
        let rtxn = self.env.read_txn().map_err(lmdb_app_error)?;
        let mut iter = self.db.iter(&rtxn).map_err(lmdb_app_error)?;
        let mut states = BTreeMap::new();
        while let Some((key, record)) = iter.next().transpose().map_err(lmdb_app_error)? {
            states.insert(key.to_string(), record);
        }
        Ok(states)
    }

    async fn raw_state(&self, ref_: &str) -> Result<Option<AdminSupplierStateRecord>, AppError> {
        self.migrate_legacy_if_needed().await?;
        let rtxn = self.env.read_txn().map_err(lmdb_app_error)?;
        self.db.get(&rtxn, ref_.trim()).map_err(lmdb_app_error)
    }
}

#[async_trait]
impl AdminStatePort for LmdbAdminSupplierStateStore {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        self.raw_states()
            .await
            .map_err(|_| AdminPortError::LookupFailed)
            .map(records_to_admin_states)
    }

    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
        self.migrate_legacy_if_needed()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        let record = AdminSupplierStateRecord::from_admin_state(state);
        let _guard = self.write_lock.lock().await;
        let mut wtxn = self
            .env
            .write_txn()
            .map_err(|_| AdminPortError::LookupFailed)?;
        self.db
            .put(&mut wtxn, ref_.trim(), &record)
            .map_err(|_| AdminPortError::LookupFailed)?;
        wtxn.commit().map_err(|_| AdminPortError::LookupFailed)
    }
}

#[async_trait]
impl AdminAccessStateLookup for LmdbAdminSupplierStateStore {
    async fn list_states(&self) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError> {
        self.raw_states()
            .await
            .map_err(|_| AuthPortError::LookupFailed)
            .map(records_to_access_states)
    }
}

#[async_trait]
impl WerkaSupplierAdminStateLookup for LmdbAdminSupplierStateStore {
    async fn werka_supplier_admin_state(
        &self,
        supplier_ref: &str,
    ) -> Result<WerkaSupplierAdminState, WerkaPortError> {
        let Some(state) = self
            .raw_state(supplier_ref)
            .await
            .map_err(|_| WerkaPortError::LookupFailed)?
        else {
            return Ok(WerkaSupplierAdminState::default());
        };
        Ok(WerkaSupplierAdminState {
            blocked: state.blocked,
            removed: state.removed,
            assigned_item_codes: state.assigned_item_codes,
        })
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct AdminSupplierStateRecord {
    #[serde(default)]
    custom_code: String,
    #[serde(default)]
    blocked: bool,
    #[serde(default)]
    removed: bool,
    #[serde(default)]
    assignments_configured: bool,
    #[serde(default)]
    assigned_item_codes: Vec<String>,
    #[serde(default)]
    pending_persist_code: String,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pending_persist_at: Option<time::OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    regen_window_started_at: Option<time::OffsetDateTime>,
    #[serde(default)]
    regen_window_count: i32,
    #[serde(default, with = "time::serde::rfc3339::option")]
    cooldown_until: Option<time::OffsetDateTime>,
}

impl AdminSupplierStateRecord {
    fn from_admin_state(state: AdminState) -> Self {
        Self {
            custom_code: state.custom_code,
            blocked: state.blocked,
            removed: state.removed,
            assignments_configured: state.assignments_configured,
            assigned_item_codes: state.assigned_item_codes,
            pending_persist_code: state.pending_persist_code,
            pending_persist_at: state.pending_persist_at,
            regen_window_started_at: state.regen_window_started_at,
            regen_window_count: state.regen_window_count,
            cooldown_until: state.cooldown_until,
        }
    }

    fn into_admin_state(self) -> AdminState {
        AdminState {
            custom_code: self.custom_code,
            blocked: self.blocked,
            removed: self.removed,
            assigned_item_codes: self.assigned_item_codes,
            cooldown_until: self.cooldown_until,
            regen_window_started_at: self.regen_window_started_at,
            regen_window_count: self.regen_window_count,
            pending_persist_code: self.pending_persist_code,
            pending_persist_at: self.pending_persist_at,
            assignments_configured: self.assignments_configured,
        }
    }
}

fn records_to_admin_states(
    raw: BTreeMap<String, AdminSupplierStateRecord>,
) -> BTreeMap<String, AdminState> {
    raw.into_iter()
        .map(|(key, value)| (key, value.into_admin_state()))
        .collect()
}

fn records_to_access_states(
    raw: BTreeMap<String, AdminSupplierStateRecord>,
) -> BTreeMap<String, AdminAccessState> {
    raw.into_iter()
        .map(|(key, value)| {
            (
                key,
                AdminAccessState {
                    custom_code: value.custom_code,
                    blocked: value.blocked,
                    removed: value.removed,
                },
            )
        })
        .collect()
}

fn decode_timestamp(timestamp: Option<i128>) -> Result<Option<time::OffsetDateTime>, BoxedError> {
    timestamp
        .map(time::OffsetDateTime::from_unix_timestamp_nanos)
        .transpose()
        .map_err(Into::into)
}

fn lmdb_app_error(error: heed::Error) -> AppError {
    AppError::Storage(format!("lmdb admin supplier state store failed: {error}"))
}
