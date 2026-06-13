use time::OffsetDateTime;

use crate::core::push::models::PushTokenRecord;
use crate::core::push::ports::PushTokenStorePort;
use crate::store::push_token_store::LmdbPushTokenStore;

#[tokio::test]
async fn lmdb_push_token_store_round_trips_records() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LmdbPushTokenStore::open(dir.path().join("push.lmdb"), 1024 * 1024, None)
        .expect("lmdb push store");

    store
        .move_token_to_key("supplier:SUP-001", "device-a", "ios")
        .await
        .expect("register");

    let records = store.list("supplier:SUP-001").await.expect("list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].token, "device-a");
    assert_eq!(records[0].platform, "ios");
}

#[tokio::test]
async fn lmdb_push_token_store_moves_token_between_owners() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LmdbPushTokenStore::open(dir.path().join("push.lmdb"), 1024 * 1024, None)
        .expect("lmdb push store");

    store
        .move_token_to_key("supplier:SUP-001", "device-a", "ios")
        .await
        .expect("register device");
    store
        .move_token_to_key("supplier:SUP-001", "shared", "ios")
        .await
        .expect("register shared");
    store
        .move_token_to_key("werka:werka", "shared", "android")
        .await
        .expect("move shared");

    let supplier = store.list("supplier:SUP-001").await.expect("supplier");
    let werka = store.list("werka:werka").await.expect("werka");
    assert_eq!(supplier.len(), 1);
    assert_eq!(supplier[0].token, "device-a");
    assert_eq!(werka.len(), 1);
    assert_eq!(werka[0].token, "shared");
    assert_eq!(werka[0].platform, "android");
}

#[tokio::test]
async fn lmdb_push_token_store_duplicate_register_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LmdbPushTokenStore::open(dir.path().join("push.lmdb"), 1024 * 1024, None)
        .expect("lmdb push store");

    store
        .move_token_to_key("werka:werka", "device-a", "android")
        .await
        .expect("register");
    let first = store.list("werka:werka").await.expect("first list");

    store
        .move_token_to_key("werka:werka", "device-a", "android")
        .await
        .expect("register duplicate");
    let duplicate = store.list("werka:werka").await.expect("duplicate list");

    assert_eq!(duplicate.len(), 1);
    assert_eq!(duplicate[0].token, "device-a");
    assert_eq!(duplicate[0].platform, "android");
    assert_eq!(duplicate[0].updated_at, first[0].updated_at);
}

#[tokio::test]
async fn lmdb_push_token_store_duplicate_register_updates_changed_platform() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LmdbPushTokenStore::open(dir.path().join("push.lmdb"), 1024 * 1024, None)
        .expect("lmdb push store");

    store
        .move_token_to_key("werka:werka", "device-a", "ios")
        .await
        .expect("register");
    store
        .move_token_to_key("werka:werka", "device-a", "android")
        .await
        .expect("update platform");

    let records = store.list("werka:werka").await.expect("list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].token, "device-a");
    assert_eq!(records[0].platform, "android");
}

#[tokio::test]
async fn lmdb_push_token_store_deletes_only_matching_owner() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LmdbPushTokenStore::open(dir.path().join("push.lmdb"), 1024 * 1024, None)
        .expect("lmdb push store");

    store
        .move_token_to_key("supplier:SUP-001", "shared", "ios")
        .await
        .expect("register");
    store
        .move_token_to_key("werka:werka", "shared", "android")
        .await
        .expect("move");
    store
        .delete("supplier:SUP-001", "shared")
        .await
        .expect("delete stale owner");

    let werka = store.list("werka:werka").await.expect("werka");
    assert_eq!(werka.len(), 1);
    assert_eq!(werka[0].token, "shared");
}

#[tokio::test]
async fn lmdb_push_token_store_migrates_legacy_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let json_path = dir.path().join("push.json");
    let updated_at = OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("timestamp");
    let raw = serde_json::to_vec(&serde_json::json!({
        "supplier:SUP-001": [
            PushTokenRecord {
                token: "device-a".into(),
                platform: "ios".into(),
                updated_at,
            }
        ]
    }))
    .expect("json");
    tokio::fs::write(&json_path, raw).await.expect("write json");

    let lmdb_path = dir.path().join("push.lmdb");
    let store = LmdbPushTokenStore::open(lmdb_path.clone(), 1024 * 1024, Some(json_path.clone()))
        .expect("lmdb push store");
    let records = store.list("supplier:SUP-001").await.expect("list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].token, "device-a");
    drop(store);

    tokio::fs::remove_file(json_path)
        .await
        .expect("remove json");
    let reloaded = LmdbPushTokenStore::open(lmdb_path, 1024 * 1024, None).expect("reload lmdb");
    let records = reloaded.list("supplier:SUP-001").await.expect("list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].token, "device-a");
    assert_eq!(records[0].platform, "ios");
}
