use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use tokio::sync::RwLock;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::models::SessionRecord;
use crate::core::session::store::{JsonSessionStore, LmdbSessionStore, SessionStore};
use crate::error::AppError;

#[derive(Clone)]
pub struct SessionManager {
    store: Arc<dyn SessionStore>,
    ttl_seconds: Option<u64>,
    cache: Arc<RwLock<HashMap<String, SessionRecord>>>,
}

impl SessionManager {
    pub fn persistent(path: PathBuf, ttl_seconds: Option<u64>) -> Self {
        Self::with_store(Arc::new(JsonSessionStore::persistent(path)), ttl_seconds)
    }

    pub fn lmdb(
        path: PathBuf,
        map_size_bytes: usize,
        ttl_seconds: Option<u64>,
    ) -> Result<Self, AppError> {
        Ok(Self::with_store(
            Arc::new(LmdbSessionStore::open(path, map_size_bytes)?),
            ttl_seconds,
        ))
    }

    #[allow(dead_code)]
    pub fn memory(ttl_seconds: Option<u64>) -> Self {
        Self::with_store(Arc::new(JsonSessionStore::memory()), ttl_seconds)
    }

    pub fn with_store(store: Arc<dyn SessionStore>, ttl_seconds: Option<u64>) -> Self {
        Self {
            store,
            ttl_seconds,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[allow(dead_code)]
    pub async fn create(&self, principal: Principal) -> Result<String, AppError> {
        let token = generate_token();
        let now = time::OffsetDateTime::now_utc();
        let record = SessionRecord::new(principal, now, None, self.ttl_seconds);
        self.store.put(&token, record.clone()).await?;
        self.cache.write().await.insert(token.clone(), record);
        Ok(token)
    }

    pub async fn get(&self, token: &str) -> Result<Principal, AppError> {
        if let Some(record) = self.cache.read().await.get(token).cloned() {
            if record.is_expired(time::OffsetDateTime::now_utc()) {
                self.delete(token).await;
                return Err(AppError::Unauthorized);
            }
            return Ok(record.principal);
        }

        let Some(record) = self.store.get(token).await? else {
            return Err(AppError::Unauthorized);
        };

        if record.is_expired(time::OffsetDateTime::now_utc()) {
            self.store.delete(token).await?;
            return Err(AppError::Unauthorized);
        }

        let principal = record.principal.clone();
        self.cache.write().await.insert(token.to_string(), record);
        Ok(principal)
    }

    pub async fn delete(&self, token: &str) {
        let _ = self.store.delete(token).await;
        self.cache.write().await.remove(token);
    }

    pub async fn delete_for_principal(
        &self,
        role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<usize, AppError> {
        let principal_ref = principal_ref.trim();
        if principal_ref.is_empty() {
            return Ok(0);
        }
        let deleted = self.store.delete_for_principal(role, principal_ref).await?;
        self.cache.write().await.retain(|_, record| {
            record.principal.role != *role || record.principal.ref_.trim() != principal_ref
        });
        Ok(deleted)
    }

    pub async fn update(&self, token: &str, principal: Principal) {
        let existing = if let Some(record) = self.cache.read().await.get(token).cloned() {
            Some(record)
        } else {
            self.store.get(token).await.ok().flatten()
        };
        let Some(existing) = existing else {
            return;
        };

        let now = time::OffsetDateTime::now_utc();
        let record = SessionRecord::new(principal, now, existing.created_at, self.ttl_seconds);
        if self.store.put(token, record.clone()).await.is_ok() {
            self.cache.write().await.insert(token.to_string(), record);
        }
    }
}

fn generate_token() -> String {
    let mut bytes = [0_u8; 24];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::{SessionManager, generate_token};
    use crate::core::auth::models::{Principal, PrincipalRole};

    #[test]
    fn token_matches_go_length() {
        assert_eq!(generate_token().len(), 32);
    }

    #[tokio::test]
    async fn update_replaces_principal() {
        let sessions = SessionManager::memory(Some(60));
        let token = sessions
            .create(Principal {
                role: PrincipalRole::Admin,
                display_name: "Admin".to_string(),
                legal_name: "Admin".to_string(),
                ref_: "admin".to_string(),
                phone: "+998880000000".to_string(),
                avatar_url: String::new(),
            })
            .await
            .expect("create session");

        sessions
            .update(
                &token,
                Principal {
                    role: PrincipalRole::Admin,
                    display_name: "Alias".to_string(),
                    legal_name: "Admin".to_string(),
                    ref_: "admin".to_string(),
                    phone: "+998880000000".to_string(),
                    avatar_url: String::new(),
                },
            )
            .await;

        let principal = sessions.get(&token).await.expect("get session");
        assert_eq!(principal.display_name, "Alias");
    }

    #[tokio::test]
    async fn lmdb_store_round_trips_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lmdb_path = dir.path().join("sessions.lmdb");
        let sessions =
            SessionManager::lmdb(lmdb_path.clone(), 1024 * 1024, Some(60)).expect("lmdb sessions");
        let token = sessions
            .create(Principal {
                role: PrincipalRole::Admin,
                display_name: "Admin".to_string(),
                legal_name: "Admin".to_string(),
                ref_: "admin".to_string(),
                phone: "+998880000000".to_string(),
                avatar_url: String::new(),
            })
            .await
            .expect("create session");

        let principal = sessions.get(&token).await.expect("get session");
        assert_eq!(principal.ref_, "admin");

        let data_file = std::fs::read(lmdb_path.join("data.mdb")).expect("read lmdb data file");
        assert!(!contains_bytes(&data_file, token.as_bytes()));

        sessions.delete(&token).await;
        assert!(sessions.get(&token).await.is_err());
    }

    #[tokio::test]
    async fn deleting_principal_sessions_revokes_only_that_identity() {
        let sessions = SessionManager::memory(Some(60));
        let worker_token = sessions
            .create(worker_principal("worker-deactivated"))
            .await
            .expect("worker session");
        let other_token = sessions
            .create(worker_principal("worker-active"))
            .await
            .expect("other session");

        let deleted = sessions
            .delete_for_principal(&PrincipalRole::Aparatchi, "worker-deactivated")
            .await
            .expect("revoke sessions");

        assert_eq!(deleted, 1);
        assert!(sessions.get(&worker_token).await.is_err());
        assert_eq!(
            sessions
                .get(&other_token)
                .await
                .expect("active session")
                .ref_,
            "worker-active"
        );
    }

    fn worker_principal(ref_: &str) -> Principal {
        Principal {
            role: PrincipalRole::Aparatchi,
            display_name: "Worker".to_string(),
            legal_name: "Worker".to_string(),
            ref_: ref_.to_string(),
            phone: "+998901112233".to_string(),
            avatar_url: String::new(),
        }
    }

    fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }
}
