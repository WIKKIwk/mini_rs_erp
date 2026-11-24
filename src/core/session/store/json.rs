use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::SessionStore;
use crate::core::auth::models::PrincipalRole;
use crate::core::session::models::SessionRecord;
use crate::error::AppError;
use crate::store::{json_file, private_file};

type Sessions = BTreeMap<String, SessionRecord>;

#[derive(Clone)]
pub struct JsonSessionStore(Backend);

#[derive(Clone)]
enum Backend {
    Persistent(PathBuf),
    Memory(Arc<Mutex<Sessions>>),
}

impl JsonSessionStore {
    pub fn persistent(path: PathBuf) -> Self {
        Self(Backend::Persistent(path))
    }

    pub fn memory() -> Self {
        Self(Backend::Memory(Arc::default()))
    }

    async fn read<T>(&self, select: impl FnOnce(&Sessions) -> T) -> Result<T, AppError> {
        match &self.0 {
            Backend::Memory(state) => {
                let sessions = state.lock().await;
                Ok(select(&sessions))
            }
            Backend::Persistent(path) => {
                let sessions = json_file::read_map(path).await?;
                Ok(select(&sessions))
            }
        }
    }

    /// Return the number of changed records. For files, keep the entire
    /// read-modify-replace operation under one OS lock, including on cancellation.
    async fn mutate(
        &self,
        update: impl FnOnce(&mut Sessions) -> usize + Send + 'static,
    ) -> Result<usize, AppError> {
        match &self.0 {
            Backend::Memory(state) => {
                let mut sessions = state.lock().await;
                Ok(update(&mut sessions))
            }
            Backend::Persistent(path) => {
                let path = path.clone();
                tokio::task::spawn_blocking(move || {
                    let _writer = lock_snapshot(&path)?;
                    let mut sessions = match std::fs::read(&path) {
                        Ok(bytes) => serde_json::from_slice(&bytes)?,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Sessions::new(),
                        Err(error) => return Err(AppError::Io(error)),
                    };
                    let changed = update(&mut sessions);
                    if changed > 0 {
                        let now = time::OffsetDateTime::now_utc();
                        // The file can contain records written by managers with different TTLs.
                        sessions.retain(|_, record| !record.is_expired(now, None));
                        private_file::write_private_sync(&path, &serde_json::to_vec_pretty(&sessions)?)?;
                    }
                    Ok(changed)
                })
                .await
                .map_err(std::io::Error::other)?
            }
        }
    }

    async fn find_by_digest(
        &self,
        prefix: &[u8],
    ) -> Result<Option<([u8; 32], SessionRecord)>, AppError> {
        self.read(|sessions| {
            let mut matches = sessions.iter().filter_map(|(token, record)| {
                let digest = super::session_key(token);
                digest.starts_with(prefix).then_some((digest, record))
            });
            let (digest, record) = matches.next()?;
            matches.next().is_none().then(|| (digest, record.clone()))
        }).await
    }
}

fn lock_snapshot(path: &Path) -> std::io::Result<File> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    // The JSON inode changes at rename. All writers lock this stable sibling instead.
    let mut lock_path = path.as_os_str().to_owned();
    lock_path.push(".lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(lock_path)?;
    lock.lock()?;
    Ok(lock)
}

#[async_trait]
impl SessionStore for JsonSessionStore {
    async fn get(&self, token: &str) -> Result<Option<SessionRecord>, AppError> {
        self.read(|sessions| sessions.get(token).cloned()).await
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
        let token = token.to_owned();
        let record = record.clone();
        self.mutate(move |sessions| {
            sessions.insert(token, record);
            1
        }).await?;
        Ok(())
    }

    async fn delete(&self, token: &str) -> Result<(), AppError> {
        let token = token.to_owned();
        self.mutate(move |sessions| usize::from(sessions.remove(&token).is_some())).await?;
        Ok(())
    }

    async fn delete_if_expired(
        &self,
        digest: &[u8; 32],
        now: time::OffsetDateTime,
        ttl_seconds: Option<u64>,
    ) -> Result<(), AppError> {
        let digest = *digest;
        self.mutate(move |sessions| {
            let expired = sessions.iter().find_map(|(token, record)| {
                (super::session_key(token) == digest && record.is_expired(now, ttl_seconds))
                    .then(|| token.clone())
            });
            if let Some(token) = expired {
                sessions.remove(&token);
                1
            } else {
                0
            }
        }).await?;
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
        let principal_ref = principal_ref.to_owned();
        let role = *role;
        self.mutate(move |sessions| {
            let before = sessions.len();
            sessions.retain(|_, record| {
                record.principal.role != role || record.principal.ref_.trim() != principal_ref
            });
            before - sessions.len()
        }).await
    }
}

#[cfg(test)]
#[tokio::test]
async fn independent_json_writers_preserve_renewal_logout_and_unrelated_updates() {
    use std::future::Future as _;
    use std::task::Poll;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sessions.json");
    let writer = JsonSessionStore::persistent(path.clone());
    let cleanup = JsonSessionStore::persistent(path.clone());
    let insert = JsonSessionStore::persistent(path.clone());
    let logout = JsonSessionStore::persistent(path.clone());
    let revoke = JsonSessionStore::persistent(path.clone());
    let old: SessionRecord = serde_json::from_value(serde_json::json!({
        "principal": {"role": "customer", "display_name": "Alice", "ref": "A"}
    })).unwrap();
    let now = time::OffsetDateTime::now_utc();
    let valid = SessionRecord::new(old.principal.clone(), now, None, Some(600));
    let mut revoked = valid.clone();
    revoked.principal.ref_ = "B".into();
    writer.put("renew", &old).await.unwrap();
    writer.put("logout-in-transaction", &valid).await.unwrap();
    writer.put("logout", &valid).await.unwrap();
    writer.put("revoke", &revoked).await.unwrap();
    for store in [&cleanup, &insert, &logout, &revoke] {
        assert!(store.get("renew").await.unwrap().is_some());
    }

    let renewed = valid.clone();
    let (holding_lock, locked) = tokio::sync::oneshot::channel();
    let (release, resume) = std::sync::mpsc::channel();
    let first = tokio::spawn(async move {
        writer.mutate(move |sessions| {
            holding_lock.send(()).unwrap();
            resume.recv().unwrap();
            sessions.insert("renew".into(), renewed);
            sessions.remove("logout-in-transaction");
            2
        }).await.unwrap()
    });
    locked.await.unwrap();
    let lock = OpenOptions::new().read(true).write(true)
        .open(dir.path().join("sessions.json.lock")).unwrap();
    assert!(matches!(lock.try_lock(), Err(std::fs::TryLockError::WouldBlock)));

    let digest = super::session_key("renew");
    let writers = async {
        tokio::join!(
            cleanup.delete_if_expired(&digest, now, Some(600)),
            insert.put("insert", &valid),
            logout.delete("logout"),
            revoke.delete_for_principal(&PrincipalRole::Customer, "B"),
        )
    };
    tokio::pin!(writers);
    // Submit every other writer while the first writer still owns the lock.
    std::future::poll_fn(|cx| match writers.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("concurrent writers completed during an open transaction"),
    }).await;
    release.send(()).unwrap();
    assert_eq!(first.await.unwrap(), 2);
    let (expired, put, deleted, revoked_count) = writers.await;
    expired.unwrap();
    put.unwrap();
    deleted.unwrap();
    assert_eq!(revoked_count.unwrap(), 1);

    let stored: Sessions = json_file::read_map(&path).await.unwrap();
    assert_eq!(stored.keys().map(String::as_str).collect::<Vec<_>>(), ["insert", "renew"]);
    assert_eq!(stored["renew"].expires_at, valid.expires_at);
}
