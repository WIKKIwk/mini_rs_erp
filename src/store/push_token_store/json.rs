use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use time::OffsetDateTime;
use tokio::sync::Mutex;

use crate::core::push::models::PushTokenRecord;
use crate::core::push::ports::{PushStoreError, PushTokenStorePort};
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
