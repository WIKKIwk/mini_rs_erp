use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::SessionStore;
use crate::core::auth::models::PrincipalRole;
use crate::core::session::models::SessionRecord;
use crate::error::AppError;
use crate::store::json_file;

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

    async fn find_by_digest(
        &self,
        prefix: &[u8],
    ) -> Result<Option<([u8; 32], SessionRecord)>, AppError> {
        let find = |sessions: &BTreeMap<String, SessionRecord>| {
            let mut matches = sessions.iter().filter_map(|(token, record)| {
                let digest = super::session_key(token);
                digest.starts_with(prefix).then_some((digest, record))
            });
            let (digest, record) = matches.next()?;
            matches.next().is_none().then(|| (digest, record.clone()))
        };
        // Other ERP processes can revoke a session in the persisted snapshot.
        if let Some(path) = &self.path {
            let sessions = json_file::read_map(path).await?;
            return Ok(find(&sessions));
        }
        Ok(find(&self.state.lock().await.sessions))
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

    async fn get_by_digest(&self, digest: &[u8; 32]) -> Result<Option<SessionRecord>, AppError> {
        Ok(self.find_by_digest(digest).await?.map(|(_, record)| record))
    }

    async fn ticket_session(
        &self,
        locator: &[u8; 16],
    ) -> Result<Option<([u8; 32], SessionRecord)>, AppError> {
        self.find_by_digest(locator).await
    }

    async fn put(&self, token: &str, record: &SessionRecord) -> Result<(), AppError> {
        let mut state = self.state.lock().await;
        self.load_if_needed(&mut state).await?;
        state.sessions.insert(token.to_string(), record.clone());
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

    async fn delete_for_principal(
        &self,
        role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<usize, AppError> {
        let principal_ref = principal_ref.trim();
        if principal_ref.is_empty() {
            return Ok(0);
        }
        let mut state = self.state.lock().await;
        self.load_if_needed(&mut state).await?;
        let previous_len = state.sessions.len();
        state.sessions.retain(|_, record| {
            record.principal.role != *role || record.principal.ref_.trim() != principal_ref
        });
        let deleted = previous_len.saturating_sub(state.sessions.len());
        if deleted > 0 {
            self.save(&state).await?;
        }
        Ok(deleted)
    }
}
