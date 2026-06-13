use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use heed::types::Bytes;
use heed::{BoxedError, BytesDecode, BytesEncode, Database, Env, EnvOpenOptions};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::core::profile::ports::{ProfilePrefs, ProfileStoreError, ProfileStorePort};
use crate::error::AppError;
use crate::store::json_file::{read_map, write_pretty};

#[derive(Clone)]
pub struct ProfileStore {
    path: PathBuf,
    state: Arc<Mutex<ProfileStoreState>>,
}

#[derive(Default)]
struct ProfileStoreState {
    loaded: bool,
    cache: HashMap<String, ProfilePrefs>,
}

impl ProfileStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: Arc::new(Mutex::new(ProfileStoreState::default())),
        }
    }
}

#[async_trait]
impl ProfileStorePort for ProfileStore {
    async fn get(&self, key: &str) -> Result<ProfilePrefs, ProfileStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        Ok(state.cache.get(key).cloned().unwrap_or_default())
    }

    async fn put(&self, key: &str, prefs: ProfilePrefs) -> Result<(), ProfileStoreError> {
        let mut state = self.state.lock().await;
        load_if_needed(&self.path, &mut state).await?;
        state.cache.insert(key.to_string(), prefs);
        write_pretty(&self.path, &state.cache)
            .await
            .map_err(|_| ProfileStoreError::StoreFailed)?;
        Ok(())
    }
}

async fn load_if_needed(
    path: &Path,
    state: &mut ProfileStoreState,
) -> Result<(), ProfileStoreError> {
    if state.loaded {
        return Ok(());
    }
    let data = read_map::<ProfilePrefs>(path)
        .await
        .map_err(|_| ProfileStoreError::StoreFailed)?;
    state.cache = data.into_iter().collect();
    state.loaded = true;
    Ok(())
}

pub struct LmdbProfileStore {
    env: Env,
    db: Database<Bytes, ProfilePrefsCodec>,
    legacy_json_path: Option<PathBuf>,
    legacy_state: Arc<Mutex<ProfileStoreState>>,
    write_lock: Arc<Mutex<()>>,
}

struct ProfilePrefsCodec;

const PROFILE_PREFS_MAGIC: &[u8] = b"AMP1";

impl<'a> BytesEncode<'a> for ProfilePrefsCodec {
    type EItem = ProfilePrefs;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let payload = bincode::serialize(&StoredProfilePrefs::from_prefs(item))?;
        let mut bytes = Vec::with_capacity(PROFILE_PREFS_MAGIC.len() + payload.len());
        bytes.extend_from_slice(PROFILE_PREFS_MAGIC);
        bytes.extend_from_slice(&payload);
        Ok(Cow::Owned(bytes))
    }
}

impl<'a> BytesDecode<'a> for ProfilePrefsCodec {
    type DItem = ProfilePrefs;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if let Some(payload) = bytes.strip_prefix(PROFILE_PREFS_MAGIC) {
            let stored: StoredProfilePrefs = bincode::deserialize(payload)?;
            return Ok(stored.into_prefs());
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[derive(Serialize, Deserialize)]
struct StoredProfilePrefs {
    nickname: String,
    avatar_url: String,
}

impl StoredProfilePrefs {
    fn from_prefs(prefs: &ProfilePrefs) -> Self {
        Self {
            nickname: prefs.nickname.clone(),
            avatar_url: prefs.avatar_url.clone(),
        }
    }

    fn into_prefs(self) -> ProfilePrefs {
        ProfilePrefs {
            nickname: self.nickname,
            avatar_url: self.avatar_url,
        }
    }
}

impl LmdbProfileStore {
    pub fn open(
        path: PathBuf,
        map_size_bytes: usize,
        legacy_json_path: Option<PathBuf>,
    ) -> Result<Self, AppError> {
        std::fs::create_dir_all(&path)?;
        let map_size = map_size_bytes.max(1024 * 1024);
        let env = unsafe {
            // This LMDB directory is owned by the profile preference store.
            EnvOpenOptions::new()
                .map_size(map_size)
                .max_dbs(1)
                .open(&path)
        }
        .map_err(lmdb_app_error)?;
        let mut wtxn = env.write_txn().map_err(lmdb_app_error)?;
        let db = env
            .create_database(&mut wtxn, Some("profile_prefs"))
            .map_err(lmdb_app_error)?;
        wtxn.commit().map_err(lmdb_app_error)?;

        Ok(Self {
            env,
            db,
            legacy_json_path,
            legacy_state: Arc::new(Mutex::new(ProfileStoreState::default())),
            write_lock: Arc::new(Mutex::new(())),
        })
    }

    async fn legacy_get(&self, key: &str) -> Result<Option<ProfilePrefs>, ProfileStoreError> {
        let Some(path) = &self.legacy_json_path else {
            return Ok(None);
        };
        let mut state = self.legacy_state.lock().await;
        load_if_needed(path, &mut state).await?;
        Ok(state.cache.get(key).cloned())
    }
}

#[async_trait]
impl ProfileStorePort for LmdbProfileStore {
    async fn get(&self, key: &str) -> Result<ProfilePrefs, ProfileStoreError> {
        let prefs = {
            let rtxn = self.env.read_txn().map_err(lmdb_store_error)?;
            self.db
                .get(&rtxn, key.as_bytes())
                .map_err(lmdb_store_error)?
        };
        if let Some(prefs) = prefs {
            return Ok(prefs);
        }

        if let Some(prefs) = self.legacy_get(key).await? {
            self.put(key, prefs.clone()).await?;
            return Ok(prefs);
        }

        Ok(ProfilePrefs::default())
    }

    async fn put(&self, key: &str, prefs: ProfilePrefs) -> Result<(), ProfileStoreError> {
        let _guard = self.write_lock.lock().await;
        let mut wtxn = self.env.write_txn().map_err(lmdb_store_error)?;
        self.db
            .put(&mut wtxn, key.as_bytes(), &prefs)
            .map_err(lmdb_store_error)?;
        wtxn.commit().map_err(lmdb_store_error)
    }
}

fn lmdb_app_error(error: heed::Error) -> AppError {
    AppError::Storage(format!("lmdb profile store failed: {error}"))
}

fn lmdb_store_error(_: heed::Error) -> ProfileStoreError {
    ProfileStoreError::StoreFailed
}

#[cfg(test)]
mod tests {
    use super::{LmdbProfileStore, ProfileStorePort};
    use crate::core::profile::ports::ProfilePrefs;

    #[tokio::test]
    async fn lmdb_profile_store_round_trips_profile_prefs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LmdbProfileStore::open(dir.path().join("profiles.lmdb"), 1024 * 1024, None)
            .expect("lmdb profile store");

        store
            .put(
                "supplier:SUP-001",
                ProfilePrefs {
                    nickname: "Ali".into(),
                    avatar_url: "https://example.test/avatar.png".into(),
                },
            )
            .await
            .expect("put prefs");

        let prefs = store.get("supplier:SUP-001").await.expect("get prefs");
        assert_eq!(prefs.nickname, "Ali");
        assert_eq!(prefs.avatar_url, "https://example.test/avatar.png");
    }

    #[tokio::test]
    async fn lmdb_profile_store_lazily_migrates_legacy_json_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let json_path = dir.path().join("profiles.json");
        tokio::fs::write(
            &json_path,
            r#"{"supplier:SUP-001":{"nickname":"Ali","avatar_url":"https://example.test/a.png"}}"#,
        )
        .await
        .expect("write legacy json");

        let lmdb_path = dir.path().join("profiles.lmdb");
        let store = LmdbProfileStore::open(lmdb_path.clone(), 1024 * 1024, Some(json_path.clone()))
            .expect("lmdb profile store");
        let prefs = store.get("supplier:SUP-001").await.expect("get prefs");
        assert_eq!(prefs.nickname, "Ali");
        assert_eq!(prefs.avatar_url, "https://example.test/a.png");
        drop(store);

        tokio::fs::remove_file(json_path)
            .await
            .expect("remove legacy json");
        let reloaded = LmdbProfileStore::open(lmdb_path, 1024 * 1024, None)
            .expect("reload lmdb profile store");
        let prefs = reloaded
            .get("supplier:SUP-001")
            .await
            .expect("get migrated prefs");
        assert_eq!(prefs.nickname, "Ali");
        assert_eq!(prefs.avatar_url, "https://example.test/a.png");
    }
}
