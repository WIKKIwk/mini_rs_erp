use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mini_rs_erp::core::auth::models::{Principal, PrincipalRole};
use mini_rs_erp::core::rps_batch::models::{
    RpsBatchResponse, RpsBatchSession, RpsBatchStartRequest, is_valid_batch_code,
    legacy_batch_code, new_batch_code,
};
use mini_rs_erp::core::rps_batch::ports::{RpsBatchStoreError, RpsBatchStorePort};
use mini_rs_erp::core::rps_batch::{RpsBatchLmdbStore, RpsBatchService};

const BATCH_CODE_MIGRATION: &str =
    include_str!("../migrations/postgres/0022_rps_batch_codes.sql");

#[test]
fn batch_code_is_24_hex_and_serialized_without_replacing_internal_id() {
    let batch_code = new_batch_code();
    assert!(is_valid_batch_code(&batch_code));

    let response = RpsBatchResponse::new(RpsBatchSession {
        id: "internal-session-id".to_string(),
        batch_code: batch_code.clone(),
        ..RpsBatchSession::default()
    });
    let json = serde_json::to_value(response).expect("serialize response");

    assert_eq!(json["batch"]["id"], "internal-session-id");
    assert_eq!(json["batch"]["batch_code"], batch_code);
}

#[test]
fn postgres_batch_code_migration_is_additive_and_backfills_json_payloads() {
    let sql = BATCH_CODE_MIGRATION.to_ascii_lowercase();
    assert!(sql.contains("create table if not exists mini_rps_batch_identities"));
    assert!(sql.contains("batch_code char(24) primary key"));
    assert!(sql.contains("unique (owner_key, batch_id)"));
    assert!(sql.contains("jsonb_set"));
    assert!(!sql.contains("delete from"));
    assert!(!sql.contains("drop table"));
    assert!(!sql.contains("truncate"));
}

#[tokio::test]
async fn lmdb_persists_legacy_codes_and_rejects_cross_batch_reuse() {
    let directory = tempfile::tempdir().expect("tempdir");
    let store = RpsBatchLmdbStore::open(directory.path().join("batch.lmdb"), 1024 * 1024)
        .expect("LMDB store");
    let batch_code = legacy_batch_code("material_taminotchi:M-1", "batch-1");
    let first = RpsBatchSession {
        id: "batch-1".to_string(),
        batch_code: batch_code.clone(),
        owner_key: "material_taminotchi:M-1".to_string(),
        ..RpsBatchSession::default()
    };

    store.put(first).await.expect("persist first batch");
    let stored = store
        .get("material_taminotchi:M-1")
        .await
        .expect("read first batch")
        .expect("stored batch");
    assert_eq!(stored.batch_code, batch_code);

    let duplicate = RpsBatchSession {
        id: "batch-2".to_string(),
        batch_code,
        owner_key: "material_taminotchi:M-2".to_string(),
        ..RpsBatchSession::default()
    };
    assert_eq!(
        store.put(duplicate).await,
        Err(RpsBatchStoreError::StoreFailed)
    );
}

#[derive(Default)]
struct MemoryBatchStore {
    batch: Mutex<Option<RpsBatchSession>>,
}

#[async_trait]
impl RpsBatchStorePort for MemoryBatchStore {
    async fn get(&self, owner_key: &str) -> Result<Option<RpsBatchSession>, RpsBatchStoreError> {
        Ok(self
            .batch
            .lock()
            .expect("batch lock")
            .as_ref()
            .filter(|batch| batch.owner_key == owner_key.trim())
            .cloned())
    }

    async fn put(&self, batch: RpsBatchSession) -> Result<(), RpsBatchStoreError> {
        *self.batch.lock().expect("batch lock") = Some(batch);
        Ok(())
    }

    async fn complete(&self, batch: RpsBatchSession) -> Result<(), RpsBatchStoreError> {
        self.put(batch).await
    }

    async fn list_completed(
        &self,
        _owner_key: &str,
        _limit: usize,
    ) -> Result<Vec<RpsBatchSession>, RpsBatchStoreError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn service_generates_batch_code_once_and_preserves_it_through_stop() {
    let service = RpsBatchService::new(Arc::new(MemoryBatchStore::default()));
    let principal = Principal {
        role: PrincipalRole::MaterialTaminotchi,
        display_name: "Materialchi".to_string(),
        legal_name: String::new(),
        ref_: "M-1".to_string(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let started = service
        .start(
            &principal,
            RpsBatchStartRequest {
                driver_url: "usb://local".to_string(),
                item_code: "ITEM-1".to_string(),
                item_name: "Green Tea".to_string(),
                warehouse: "Stores - A".to_string(),
                ..RpsBatchStartRequest::default()
            },
        )
        .await
        .expect("start batch")
        .batch;
    assert!(is_valid_batch_code(&started.batch_code));

    let current = service.state(&principal).await.expect("batch state").batch;
    assert_eq!(current.batch_code, started.batch_code);

    let stopped = service.stop(&principal).await.expect("stop batch").batch;
    assert!(!stopped.active);
    assert_eq!(stopped.batch_code, started.batch_code);
    assert_eq!(stopped.id, started.id);
}
