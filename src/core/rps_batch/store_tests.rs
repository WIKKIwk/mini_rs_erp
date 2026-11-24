use super::*;

#[test]
fn order_commitment_round_trips_and_reads_previous_binary_session() {
    let batch = RpsBatchSession {
        id: "linked-batch".into(), batch_code: "12345".into(), revision: 1,
        order_assignment: Some(serde_json::json!({"order_id":"zakaz-1","apparatus":"apparatus:default:bosma_7"})),
        ..Default::default()
    };
    let encoded = RpsBatchSessionCodec::bytes_encode(&batch).unwrap();
    assert_eq!(RpsBatchSessionCodec::bytes_decode(&encoded).unwrap(), batch);
    let legacy = RpsBatchSessionV5 {
        id: "legacy".into(), batch_code: "12345".into(), revision: 3, length_m: Some(125.0),
        ..Default::default()
    };
    let mut bytes = RPS_BATCH_MAGIC.to_vec();
    bytes.extend(bincode::serialize(&legacy).unwrap());
    let restored = RpsBatchSessionCodec::bytes_decode(&bytes).unwrap();
    assert_eq!(restored.id, "legacy");
    assert_eq!(restored.length_m, Some(125.0));
    assert!(restored.order_assignment.is_none());
}

#[tokio::test]
async fn lmdb_batch_store_round_trips_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = RpsBatchLmdbStore::open(dir.path().join("batch.lmdb"), 1024 * 1024)
        .expect("lmdb store");
    let mut batch = RpsBatchSession {
        id: "batch-1".to_string(),
        active: true,
        owner_key: "werka:W-1".to_string(),
        item_code: "ITEM-1".to_string(),
        warehouse: "Stores - A".to_string(),
        ..RpsBatchSession::default()
    };
    batch.ensure_context();

    store.put(&batch).await.expect("put");

    assert_eq!(store.get("werka:W-1").await.expect("get"), Some(batch));
}

#[tokio::test]
async fn lmdb_batch_store_completes_and_lists_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = RpsBatchLmdbStore::open(dir.path().join("batch.lmdb"), 1024 * 1024)
        .expect("lmdb store");
    let mut batch = RpsBatchSession {
        id: "batch-1".to_string(),
        owner_key: "material_taminotchi:M-1".to_string(),
        owner_role: "material_taminotchi".to_string(),
        owner_ref: "M-1".to_string(),
        updated_at: "2026-07-20T05:00:00Z".to_string(),
        ..RpsBatchSession::default()
    };
    batch.ensure_context();

    store.complete(&batch).await.expect("complete");
    store.complete(&batch).await.expect("idempotent complete");

    assert_eq!(
        store
            .list_completed("material_taminotchi:M-1", 50)
            .await
            .expect("history"),
        vec![batch]
    );
}

#[tokio::test]
async fn lmdb_batch_store_rejects_batch_code_reuse() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = RpsBatchLmdbStore::open(dir.path().join("batch.lmdb"), 1024 * 1024)
        .expect("lmdb store");
    let first = RpsBatchSession {
        id: "batch-1".to_string(),
        batch_code: "421234567890ABCDEF123456".to_string(),
        owner_key: "material_taminotchi:M-1".to_string(),
        ..RpsBatchSession::default()
    };
    let second = RpsBatchSession {
        id: "batch-2".to_string(),
        batch_code: first.batch_code.clone(),
        owner_key: "material_taminotchi:M-2".to_string(),
        ..RpsBatchSession::default()
    };

    store.put(&first).await.expect("first batch");
    assert_eq!(
        store.put(&second).await,
        Err(RpsBatchStoreError::StoreFailed)
    );
}

#[test]
fn lmdb_batch_store_reads_pre_print_list_session() {
    let legacy = RpsBatchSessionV2 {
        id: "batch-v2".to_string(),
        owner_key: "material_taminotchi:M-1".to_string(),
        item_code: "ITEM-1".to_string(),
        warehouse: "Stores - A".to_string(),
        ..RpsBatchSessionV2::default()
    };
    let mut bytes = RPS_BATCH_MAGIC.to_vec();
    bytes.extend_from_slice(&bincode::serialize(&legacy).expect("serialize legacy batch"));

    let decoded = RpsBatchSessionCodec::bytes_decode(&bytes).expect("decode legacy batch");

    assert_eq!(decoded.id, "batch-v2");
    assert!(super::super::models::is_valid_batch_code(
        &decoded.batch_code
    ));
    assert!(decoded.prints.is_empty());
}

#[test]
fn lmdb_batch_store_reads_pre_length_session_without_losing_dimensions() {
    let legacy = RpsBatchSessionV4 {
        id: "batch-v4".to_string(),
        batch_code: "421234567890ABCDEF123456".to_string(),
        revision: 3,
        active: true,
        owner_key: "material_taminotchi:M-1".to_string(),
        item_code: "CPP".to_string(),
        item_name: "CPP".to_string(),
        warehouse: "Stores - A".to_string(),
        width_mm: Some(1030.0),
        micron: Some(25.0),
        ..RpsBatchSessionV4::default()
    };
    let mut bytes = RPS_BATCH_MAGIC.to_vec();
    bytes.extend_from_slice(&bincode::serialize(&legacy).expect("serialize legacy batch"));

    let decoded = RpsBatchSessionCodec::bytes_decode(&bytes).expect("decode legacy batch");

    assert_eq!(decoded.id, "batch-v4");
    assert_eq!(decoded.revision, 3);
    assert_eq!(decoded.width_mm, Some(1030.0));
    assert_eq!(decoded.micron, Some(25.0));
    assert_eq!(decoded.length_m, None);
}
