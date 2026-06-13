use super::{
    BytesDecode, BytesEncode, LmdbSessionStore, SessionRecordCodec, SessionStore, session_key,
};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::models::SessionRecord;
use heed::types::Bytes;
use heed::{Database, EnvOpenOptions};

#[tokio::test]
async fn lmdb_get_migrates_legacy_raw_token_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store =
        LmdbSessionStore::open(dir.path().join("sessions.lmdb"), 1024 * 1024).expect("lmdb store");
    let token = "legacy-token";
    let record = SessionRecord::new(principal(), time::OffsetDateTime::now_utc(), None, None);

    {
        let mut wtxn = store.env.write_txn().expect("write txn");
        store
            .db
            .put(&mut wtxn, token.as_bytes(), &record)
            .expect("put legacy session");
        wtxn.commit().expect("commit legacy session");
    }

    let loaded = store.get(token).await.expect("get migrated session");
    assert_eq!(loaded.expect("session").principal.ref_, "admin");

    let key = session_key(token);
    let rtxn = store.env.read_txn().expect("read txn");
    assert!(
        store
            .db
            .get(&rtxn, token.as_bytes())
            .expect("legacy key")
            .is_none()
    );
    assert!(store.db.get(&rtxn, &key).expect("hashed key").is_some());
}

#[test]
fn session_record_codec_round_trips_binary() {
    let record = SessionRecord::new(principal(), time::OffsetDateTime::now_utc(), None, None);
    let bytes = SessionRecordCodec::bytes_encode(&record).expect("encode record");
    let decoded = SessionRecordCodec::bytes_decode(&bytes).expect("decode record");

    assert_eq!(decoded.principal.ref_, "admin");
    assert_eq!(decoded.created_at, record.created_at);
}

#[tokio::test]
async fn lmdb_reads_legacy_json_session_values() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sessions.lmdb");
    let token = "json-token";
    let key = session_key(token);
    let record = SessionRecord::new(principal(), time::OffsetDateTime::now_utc(), None, None);

    {
        std::fs::create_dir_all(&path).expect("create lmdb dir");
        let env = unsafe {
            EnvOpenOptions::new()
                .map_size(1024 * 1024)
                .max_dbs(2)
                .open(&path)
        }
        .expect("legacy env");
        let mut wtxn = env.write_txn().expect("legacy write txn");
        let db: Database<Bytes, Bytes> = env
            .create_database(&mut wtxn, Some("sessions"))
            .expect("legacy db");
        let json = serde_json::to_vec(&record).expect("legacy json");
        db.put(&mut wtxn, &key, &json).expect("put legacy json");
        wtxn.commit().expect("commit legacy json");
    }

    let store = LmdbSessionStore::open(path, 1024 * 1024).expect("lmdb store");
    let loaded = store.get(token).await.expect("read json session");

    assert_eq!(loaded.expect("session").principal.ref_, "admin");
}

#[tokio::test]
async fn lmdb_purges_expired_sessions_from_expiry_index() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store =
        LmdbSessionStore::open(dir.path().join("sessions.lmdb"), 1024 * 1024).expect("lmdb store");
    let now = time::OffsetDateTime::now_utc();

    store
        .put(
            "expired-token",
            record_with_expiry(now - time::Duration::seconds(1)),
        )
        .await
        .expect("put expired");
    assert!(
        store
            .get("expired-token")
            .await
            .expect("get before purge")
            .is_some()
    );

    store
        .put(
            "active-token",
            record_with_expiry(now + time::Duration::seconds(60)),
        )
        .await
        .expect("put active");

    assert!(
        store
            .get("expired-token")
            .await
            .expect("get expired")
            .is_none()
    );
    assert!(
        store
            .get("active-token")
            .await
            .expect("get active")
            .is_some()
    );
}

#[tokio::test]
async fn lmdb_updates_and_deletes_expiry_index_entries() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store =
        LmdbSessionStore::open(dir.path().join("sessions.lmdb"), 1024 * 1024).expect("lmdb store");
    let now = time::OffsetDateTime::now_utc();

    store
        .put(
            "moving-token",
            record_with_expiry(now + time::Duration::seconds(60)),
        )
        .await
        .expect("put first expiry");
    assert_eq!(expiry_index_len(&store), 1);

    store
        .put(
            "moving-token",
            record_with_expiry(now + time::Duration::seconds(120)),
        )
        .await
        .expect("replace expiry");
    assert_eq!(expiry_index_len(&store), 1);

    store.delete("moving-token").await.expect("delete session");
    assert_eq!(expiry_index_len(&store), 0);
}

fn principal() -> Principal {
    Principal {
        role: PrincipalRole::Admin,
        display_name: "Admin".to_string(),
        legal_name: "Admin".to_string(),
        ref_: "admin".to_string(),
        phone: "+998880000000".to_string(),
        avatar_url: String::new(),
    }
}

fn record_with_expiry(expires_at: time::OffsetDateTime) -> SessionRecord {
    SessionRecord {
        principal: principal(),
        created_at: Some(time::OffsetDateTime::now_utc()),
        updated_at: Some(time::OffsetDateTime::now_utc()),
        expires_at: Some(expires_at),
    }
}

fn expiry_index_len(store: &LmdbSessionStore) -> usize {
    let rtxn = store.env.read_txn().expect("read txn");
    let mut count = 0;
    for entry in store.expires_db.iter(&rtxn).expect("expiry iter") {
        entry.expect("expiry entry");
        count += 1;
    }
    count
}
