use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore;
use tokio::sync::RwLock;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::access::SessionAccessValidator;
use crate::core::session::models::SessionRecord;
use crate::core::session::store::{JsonSessionStore, LmdbSessionStore, SessionStore};
use crate::error::AppError;

#[derive(Clone)]
pub struct SessionManager {
    store: Arc<dyn SessionStore>,
    ttl_seconds: Option<u64>,
    cache: Arc<RwLock<HashMap<String, SessionRecord>>>,
    operations: Arc<RwLock<()>>,
    access_validator: Option<Arc<dyn SessionAccessValidator>>,
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
            operations: Arc::new(RwLock::new(())),
            access_validator: None,
        }
    }

    pub fn with_access_validator(mut self, validator: Arc<dyn SessionAccessValidator>) -> Self {
        self.access_validator = Some(validator);
        self
    }

    async fn require_active(&self, principal: &Principal) -> Result<(), AppError> {
        if let Some(validator) = &self.access_validator {
            if !validator.is_active(principal).await? {
                self.delete_principal_records(&principal.role, &principal.ref_)
                    .await?;
                return Err(AppError::Unauthorized);
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn create(&self, principal: Principal) -> Result<String, AppError> {
        let _operation = self.operations.read().await;
        self.require_active(&principal).await?;
        let token = generate_token();
        let now = time::OffsetDateTime::now_utc();
        let record = SessionRecord::new(principal, now, None, self.ttl_seconds);
        self.store.put(&token, &record).await?;
        self.cache.write().await.insert(token.clone(), record);
        Ok(token)
    }

    pub async fn get(&self, token: &str) -> Result<Principal, AppError> {
        let _operation = self.operations.read().await;
        let cached = self.cache.read().await.get(token).cloned();
        if let Some(record) = cached {
            if record.is_expired(time::OffsetDateTime::now_utc()) {
                self.delete_record(token).await?;
                return Err(AppError::Unauthorized);
            }
            self.require_active(&record.principal).await?;
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
        self.require_active(&principal).await?;
        self.cache.write().await.insert(token.to_string(), record);
        Ok(principal)
    }

    pub async fn delete(&self, token: &str) -> Result<(), AppError> {
        let _operation = self.operations.write().await;
        self.delete_record(token).await
    }

    async fn delete_record(&self, token: &str) -> Result<(), AppError> {
        self.store.delete(token).await?;
        self.cache.write().await.remove(token);
        Ok(())
    }

    pub async fn delete_for_principal(
        &self,
        role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<usize, AppError> {
        // Do not let an in-flight read/update restore a revoked token afterward.
        let _operation = self.operations.write().await;
        self.delete_principal_records(role, principal_ref).await
    }

    async fn delete_principal_records(
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

    pub async fn update(&self, token: &str, principal: Principal) -> Result<(), AppError> {
        let _operation = self.operations.read().await;
        self.require_active(&principal).await?;
        let cached = self.cache.read().await.get(token).cloned();
        let existing = if let Some(record) = cached {
            Some(record)
        } else {
            self.store.get(token).await?
        };
        let Some(existing) = existing else {
            return Err(AppError::Unauthorized);
        };

        if existing.is_expired(time::OffsetDateTime::now_utc()) {
            self.delete_record(token).await?;
            return Err(AppError::Unauthorized);
        }

        let now = time::OffsetDateTime::now_utc();
        let record = SessionRecord::new(principal, now, existing.created_at, self.ttl_seconds);
        self.store.put(token, &record).await?;
        self.cache.write().await.insert(token.to_string(), record);
        Ok(())
    }
}

fn generate_token() -> String {
    let mut bytes = [0_u8; 24];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
#[path = "revocation_tests.rs"]
mod revocation_tests;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::{SessionManager, generate_token};
    use crate::core::auth::models::{Principal, PrincipalRole};
    use crate::core::session::models::SessionRecord;
    use crate::core::session::store::SessionStore;
    use crate::error::AppError;

    struct FailingSessionStore;

    #[async_trait]
    impl SessionStore for FailingSessionStore {
        async fn get(&self, _token: &str) -> Result<Option<SessionRecord>, AppError> {
            Err(AppError::Storage("session get failed".to_string()))
        }

        async fn put(&self, _token: &str, _record: &SessionRecord) -> Result<(), AppError> {
            Err(AppError::Storage("session put failed".to_string()))
        }

        async fn delete(&self, _token: &str) -> Result<(), AppError> {
            Err(AppError::Storage("session delete failed".to_string()))
        }

        async fn delete_for_principal(
            &self,
            _role: &PrincipalRole,
            _principal_ref: &str,
        ) -> Result<usize, AppError> {
            Err(AppError::Storage("session revoke failed".to_string()))
        }
    }

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
            .await
            .expect("update session");

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

        sessions.delete(&token).await.expect("delete session");
        assert!(sessions.get(&token).await.is_err());
    }

    #[tokio::test]
    async fn session_store_failures_are_returned_to_callers() {
        let sessions = SessionManager::with_store(Arc::new(FailingSessionStore), Some(60));

        assert!(matches!(
            sessions.update("token", worker_principal("worker")).await,
            Err(AppError::Storage(message)) if message == "session get failed"
        ));
        assert!(matches!(
            sessions.delete("token").await,
            Err(AppError::Storage(message)) if message == "session delete failed"
        ));
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
