use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;

use async_trait::async_trait;
use tokio::sync::Semaphore;

use super::*;

struct PausedReadStore {
    inner: JsonSessionStore,
    pause: AtomicBool,
    read_started: Semaphore,
    resume: Semaphore,
}

#[async_trait]
impl SessionStore for PausedReadStore {
    async fn get(&self, token: &str) -> Result<Option<SessionRecord>, AppError> {
        let record = self.inner.get(token).await?;
        if self.pause.swap(false, Ordering::SeqCst) {
            self.read_started.add_permits(1);
            self.resume.acquire().await.unwrap().forget();
        }
        Ok(record)
    }

    async fn put(&self, token: &str, record: &SessionRecord) -> Result<(), AppError> {
        self.inner.put(token, record).await
    }

    async fn delete(&self, token: &str) -> Result<(), AppError> {
        self.inner.delete(token).await
    }

    async fn delete_for_principal(
        &self,
        role: &PrincipalRole,
        principal_ref: &str,
    ) -> Result<usize, AppError> {
        self.inner.delete_for_principal(role, principal_ref).await
    }
}

#[tokio::test]
async fn in_flight_read_cannot_repopulate_cache_after_revocation() {
    let store = Arc::new(PausedReadStore {
        inner: JsonSessionStore::memory(),
        pause: AtomicBool::new(false),
        read_started: Semaphore::new(0),
        resume: Semaphore::new(0),
    });
    let original = SessionManager::with_store(store.clone(), Some(60));
    let token = original
        .create(Principal {
            role: PrincipalRole::Supplier,
            ref_: "synthetic-supplier".into(),
            display_name: String::new(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .unwrap();
    // Start with an empty cache, as after a process restart.
    let sessions = SessionManager::with_store(store.clone(), Some(60));
    store.pause.store(true, Ordering::SeqCst);
    let reading = {
        let sessions = sessions.clone();
        let token = token.clone();
        tokio::spawn(async move { sessions.get(&token).await })
    };
    store.read_started.acquire().await.unwrap().forget();

    let revoking = sessions.delete_for_principal(&PrincipalRole::Supplier, "synthetic-supplier");
    tokio::pin!(revoking);
    std::future::poll_fn(|cx| match revoking.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(()),
        Poll::Ready(_) => panic!("revocation must wait for an in-flight cache fill"),
    })
    .await;

    store.resume.add_permits(1);
    reading.await.unwrap().unwrap();
    assert_eq!(revoking.await.unwrap(), 1);
    assert!(matches!(
        sessions.get(&token).await,
        Err(AppError::Unauthorized)
    ));
}
