use super::*;
use crate::core::production_map::{
    OrderRunSession, OrderRunStatus, ProductionMapDefinition, QueueActionActor,
    queue_state::ApparatusQueueOrderState,
};

const FROM: &str = "apparatus:test:source";
const TO: &str = "apparatus:test:target";
const ORDER: &str = "zakaz-requeued-sql";

fn write() -> ProductionMapApparatusTransferWrite {
    let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
        "id": ORDER, "product_code": "SQL", "title": "SQL", "nodes": [], "edges": []
    }))
    .unwrap();
    let session = OrderRunSession {
        session_id: "requeued-session".into(),
        apparatus: TO.into(),
        order_id: ORDER.into(),
        stage_node_id: "print".into(),
        status: OrderRunStatus::Paused,
        worker_role: "aparatchi".into(),
        worker_ref: "worker".into(),
        worker_display_name: "Worker".into(),
        started_at_unix: 1,
        updated_at_unix: 2,
        payload_json: serde_json::json!({"requeued_at_tail": true}),
    };
    let record = ProductionMapApparatusTransferRecord {
        transfer_id: "transfer-requeued-sql".into(),
        idempotency_key: "requeued-sql".into(),
        order_id: ORDER.into(),
        from_apparatus: FROM.into(),
        to_apparatus: TO.into(),
        reason: "Transfer after unfreeze".into(),
        actor: QueueActionActor {
            role: "admin".into(),
            ref_: "admin".into(),
            display_name: "Admin".into(),
        },
        session_id: session.session_id.clone(),
        progress_batch_id: String::new(),
        queue_state: ApparatusQueueOrderState::Pending,
        material_barcodes: vec![],
        map: map.clone(),
        session: session.clone(),
        progress_batch: None,
        progress_batch_updates: vec![],
        created_at_unix: 2,
    };
    ProductionMapApparatusTransferWrite {
        record,
        updated_map: map,
        from_sequence: vec![],
        to_sequence: vec![ORDER.into()],
        from_states: Default::default(),
        to_states: [(ORDER.into(), "pending".into())].into(),
        target_apparatus_id: TO.into(),
        session,
        progress_batch: None,
        progress_batch_updates: vec![],
        raw_material_assignments: vec![],
    }
}

// Every table is connection-local; no application records or shared schema change.
async fn pool() -> PgPool {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    sqlx::raw_sql(
        "
        CREATE TEMP TABLE mini_apparatus (id TEXT PRIMARY KEY, name TEXT);
        CREATE TEMP TABLE mini_queue_states (apparatus TEXT, canonical_apparatus_id TEXT,
            order_id TEXT, state TEXT, updated_at TIMESTAMPTZ,
            PRIMARY KEY (canonical_apparatus_id, order_id));
        CREATE TEMP TABLE mini_order_control_states (order_id TEXT PRIMARY KEY, state TEXT);
        CREATE TEMP TABLE mini_order_run_sessions (session_id TEXT PRIMARY KEY,
            canonical_apparatus_id TEXT, order_id TEXT, status TEXT, payload_json JSONB);
        CREATE TEMP TABLE mini_progress_batches (batch_id TEXT PRIMARY KEY,
            canonical_apparatus_id TEXT, order_id TEXT, session_id TEXT, action TEXT, status TEXT);
    ",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO mini_apparatus VALUES ($1, 'Source'), ($2, 'Target')")
        .bind(FROM)
        .bind(TO)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO mini_queue_states (canonical_apparatus_id, order_id, state) VALUES ($1,$2,'pending')")
        .bind(FROM).bind(ORDER).execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO mini_order_run_sessions VALUES ('requeued-session',$1,$2,'paused',$3)",
    )
    .bind(FROM)
    .bind(ORDER)
    .bind(serde_json::json!({"requeued_at_tail":true}))
    .execute(&pool)
    .await
    .unwrap();
    pool
}

#[tokio::test]
async fn postgres_requeued_transfer_rechecks_session_and_preserves_pending() {
    let pool = pool().await;
    let write = write();
    let mut tx = pool.begin().await.unwrap();
    verify_transfer_preconditions(&mut tx, &write)
        .await
        .unwrap();
    transfer_queue_state_tx(&mut tx, FROM, TO, ORDER, write.record.queue_state.as_str())
        .await
        .unwrap();
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT canonical_apparatus_id,state FROM mini_queue_states")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    assert_eq!(rows, vec![(TO.into(), "pending".into())]);
    tx.rollback().await.unwrap();
    for (status, payload, expected) in [
        (
            "paused",
            serde_json::json!({}),
            ProductionMapError::ApparatusTransferOrderNotPaused,
        ),
        (
            "active",
            serde_json::json!({"requeued_at_tail":true}),
            ProductionMapError::ApparatusTransferSessionMismatch,
        ),
    ] {
        sqlx::query("UPDATE mini_order_run_sessions SET status=$1,payload_json=$2")
            .bind(status)
            .bind(payload)
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert_eq!(
            verify_transfer_preconditions(&mut tx, &write).await,
            Err(expected)
        );
        tx.rollback().await.unwrap();
    }
    sqlx::query("UPDATE mini_order_run_sessions SET status='paused',payload_json=$1")
        .bind(serde_json::json!({"requeued_at_tail":true}))
        .execute(&pool)
        .await
        .unwrap();
    for (state, expected) in [
        ("frozen", ProductionMapError::OrderFrozen),
        ("freeze_requested", ProductionMapError::OrderFreezeRequested),
    ] {
        sqlx::query("INSERT INTO mini_order_control_states VALUES ($1,$2) ON CONFLICT(order_id) DO UPDATE SET state=$2")
            .bind(ORDER).bind(state).execute(&pool).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert_eq!(
            verify_transfer_preconditions(&mut tx, &write).await,
            Err(expected)
        );
        tx.rollback().await.unwrap();
    }
    sqlx::query("DELETE FROM mini_order_control_states")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO mini_progress_batches VALUES ('new-pause',$1,$2,'requeued-session','pause','paused')")
        .bind(FROM).bind(ORDER).execute(&pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    assert_eq!(
        verify_transfer_preconditions(&mut tx, &write).await,
        Err(ProductionMapError::ApparatusTransferProgressMismatch)
    );
    tx.rollback().await.unwrap();
    pool.close().await;
}

async fn insert_receipt(
    pool: &PgPool,
    record: &ProductionMapApparatusTransferRecord,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO mini_apparatus_order_transfers
        (transfer_id,idempotency_key,order_id,from_apparatus,to_apparatus,
         canonical_from_apparatus_id,canonical_to_apparatus_id,reason,actor_role,
         session_id,progress_batch_id,payload_json)
         VALUES ($1,$2,$3,$4,$5,$4,$5,$6,'admin',$7,NULL,$8)",
    )
    .bind(&record.transfer_id)
    .bind(&record.idempotency_key)
    .bind(&record.order_id)
    .bind(&record.from_apparatus)
    .bind(&record.to_apparatus)
    .bind(&record.reason)
    .bind(&record.session_id)
    .bind(serde_json::to_value(record).unwrap())
    .execute(pool)
    .await?;
    Ok(())
}

#[tokio::test]
async fn postgres_requeued_transfer_migration_keeps_output_free_receipts_readable() {
    let pool = pool().await;
    sqlx::raw_sql(
        &include_str!("../../../../migrations/postgres/0032_apparatus_order_transfers.sql")
            .replace(
                "CREATE TABLE IF NOT EXISTS",
                "CREATE TEMP TABLE IF NOT EXISTS",
            ),
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(
        "ALTER TABLE mini_apparatus_order_transfers
        ADD COLUMN canonical_from_apparatus_id TEXT, ADD COLUMN canonical_to_apparatus_id TEXT",
    )
    .execute(&pool)
    .await
    .unwrap();
    let record = write().record;
    assert!(insert_receipt(&pool, &record).await.is_err());
    sqlx::raw_sql(include_str!(
        "../../../../migrations/postgres/0171_requeued_apparatus_transfers.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    insert_receipt(&pool, &record).await.unwrap();
    let replay = load_apparatus_transfer_by_idempotency_key(&pool, &record.idempotency_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replay, record);
    pool.close().await;
}
