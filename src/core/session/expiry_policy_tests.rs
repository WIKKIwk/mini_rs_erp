use super::*;
use crate::core::session::scoped_ticket::{issue_ticket, TicketScope};
use time::{Duration, OffsetDateTime};

fn legacy_record() -> SessionRecord {
    serde_json::from_value(serde_json::json!({
        "principal": {
            "role": "admin", "display_name": "Administrator", "legal_name": "",
            "ref": "admin", "phone": "", "avatar_url": ""
        }
    })).unwrap()
}

#[test]
fn exact_deadline_is_expired_and_optional_ttl_preserves_nonexpiring_sessions() {
    let now = OffsetDateTime::now_utc();
    let mut record = legacy_record();
    assert!(record.is_expired(now, Some(60)));
    assert!(!record.is_expired(now, None));
    record.expires_at = Some(now);
    for policy in [Some(60), None] {
        assert!(!record.is_expired(now - Duration::nanoseconds(1), policy));
        assert!(record.is_expired(now, policy));
    }
}

#[tokio::test]
async fn restored_sessions_without_expiry_are_removed_across_access_paths() {
    let directory = tempfile::tempdir().unwrap();
    let record = legacy_record();
    let scope = TicketScope::Avatar { profile: None, version: "old" };
    for store in [
        Arc::new(JsonSessionStore::persistent(directory.path().join("sessions.json"))) as Arc<dyn SessionStore>,
        Arc::new(LmdbSessionStore::open(directory.path().join("sessions.lmdb"), 1024 * 1024).unwrap()),
    ] {
        let sessions = SessionManager::with_store(store.clone(), Some(60));
        for cached in [false, true] {
            for operation in ["get", "update", "digest", "ticket"] {
                let token = generate_token();
                let digest = session_digest(&token);
                store.put(&token, &record).await.unwrap();
                if cached {
                    sessions.cache.write().await.insert(digest, record.clone());
                }
                let result = match operation {
                    "get" => sessions.get(&token).await.map(drop),
                    "update" => sessions.update(&token, record.principal.clone()).await,
                    "digest" => sessions.get_by_digest(&digest).await.map(drop),
                    _ => sessions.resolve_ticket(&issue_ticket(&token, scope), scope).await.map(drop),
                };
                assert!(matches!(result, Err(AppError::Unauthorized)), "{cached} {operation}");
                assert!(store.get_by_digest(&digest).await.unwrap().is_none());
                assert!(!sessions.cache.read().await.contains_key(&digest));
            }
        }
        let token = generate_token();
        store.put(&token, &record).await.unwrap();
        let unlimited = SessionManager::with_store(store, None);
        assert_eq!(unlimited.get(&token).await.unwrap(), record.principal);
        unlimited.update(&token, record.principal.clone()).await.unwrap();
        assert!(unlimited.resolve_ticket(&issue_ticket(&token, scope), scope).await.is_ok());
    }
}

#[tokio::test]
async fn expiry_cleanup_preserves_a_newer_record_and_unrelated_json_sessions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sessions.json");
    let store = Arc::new(JsonSessionStore::persistent(path.clone()));
    let cached = SessionManager::with_store(store.clone(), Some(60));
    let token = generate_token();
    let digest = session_digest(&token);
    let expired = legacy_record();
    store.put(&token, &expired).await.unwrap();
    cached.cache.write().await.insert(digest, expired.clone());
    let writer = JsonSessionStore::persistent(path);
    let current = SessionRecord::new(expired.principal.clone(), OffsetDateTime::now_utc(), None, Some(60));
    writer.put(&token, &current).await.unwrap();
    writer.put("another-token", &current).await.unwrap();
    assert!(matches!(cached.get(&token).await, Err(AppError::Unauthorized)));
    assert!(store.get_by_digest(&digest).await.unwrap().is_some());
    assert!(store.get_by_digest(&session_digest("another-token")).await.unwrap().is_some());
}
