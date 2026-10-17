#![cfg(feature = "verification")]
//! Requires an explicit disposable server; never defaults to the runtime database.
use mini_rs_erp::{
    core::production_map::*,
    db::{postgres, postgres_production_map::PostgresProductionMapStore},
};
use serde_json::json;
use std::sync::Arc;

const PRODUCER: &str = "apparatus:default:bosma_7";
const DOWNSTREAM: &str = "apparatus:default:asset-007";
const WORKER: &str = "history-worker";
const ORDER: &str = "zakaz-producer-history";

fn batch(id: &str, producer: &str, worker: &str) -> OrderProgressBatch {
    serde_json::from_value(json!({
        "batch_id":id,"session_id":"history-run","apparatus":producer,"order_id":ORDER,
        "action":"complete","status":"completed","produced_qty":10,"uom":"m",
        "started_at_unix":1,"completed_at_unix":2,"label_item_code":"HISTORY",
        "label_item_name":"History","executor_name":"Worker","worker_display_name":"Worker",
        "qr_payload":format!("qr-{id}"),"worker_ref":worker,"worker_role":"aparatchi",
        "wip_status":"processed","current_apparatus":DOWNSTREAM,"next_apparatus":DOWNSTREAM,
        "payload_json": {"stage_node_id":"press", "next_stage_node_id":"lam"}
    }))
    .unwrap()
}

async fn assert_history(store: &dyn ProductionMapStorePort) {
    for index in 0..520 {
        store
            .put_order_progress_batch(batch(&format!("z-noise-{index:04}"), DOWNSTREAM, WORKER))
            .await
            .unwrap();
        store
            .put_order_progress_batch(batch(
                &format!("y-other-worker-{index:04}"),
                PRODUCER,
                "other-worker",
            ))
            .await
            .unwrap();
    }
    for index in 0..505 {
        store
            .put_order_progress_batch(batch(&format!("a-output-{index:04}"), PRODUCER, WORKER))
            .await
            .unwrap();
    }
    let mut free = batch("a-output-0000", PRODUCER, WORKER);
    free.wip_status = OrderProgressBatchWipStatus::Waiting;
    free.current_apparatus = PRODUCER.into();
    store.put_order_progress_batch(free).await.unwrap();
    let mut query = WipProgressBatchQuery::new("", "", "", None, true, ORDER, 500);
    query.producer_apparatus = PRODUCER.into();
    query.worker_ref = WORKER.into();
    let first = store.wip_progress_batches(query.clone()).await.unwrap();
    assert_eq!(first.len(), 500);
    assert!(
        first
            .iter()
            .all(|b| b.apparatus == PRODUCER && b.worker_ref == WORKER)
    );
    assert_eq!(first[0].batch_id, "a-output-0504");
    assert_eq!(first[0].current_apparatus, DOWNSTREAM);
    query.before_batch_id = first.last().unwrap().batch_id.clone();
    // Updating an earlier batch's current location/recency between reads must
    // not displace older rows across the immutable ID cursor.
    store
        .put_order_progress_batch(batch("a-output-0504", PRODUCER, WORKER))
        .await
        .unwrap();
    let second = store.wip_progress_batches(query.clone()).await.unwrap();
    assert_eq!(second.len(), 5);
    assert!(
        second
            .iter()
            .all(|b| !first.iter().any(|old| old.batch_id == b.batch_id))
    );
    let oldest = second.last().unwrap();
    assert_eq!(oldest.batch_id, "a-output-0000");
    assert_eq!(oldest.wip_status, OrderProgressBatchWipStatus::Waiting);
    query.before_batch_id = oldest.batch_id.clone();
    assert!(store.wip_progress_batches(query).await.unwrap().is_empty());
}

#[tokio::test]
async fn memory_producer_history_filters_before_limit_and_pages_to_old_free_output() {
    assert_history(&MemoryProductionMapStore::new()).await;
}

#[tokio::test]
async fn postgres_producer_history_filters_before_limit_and_pages_to_old_free_output() {
    let admin_url = std::env::var("WIP_HISTORY_TEST_DATABASE_URL")
        .expect("WIP_HISTORY_TEST_DATABASE_URL must be a disposable PostgreSQL server");
    let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
    let database = format!("wip_history_{:032x}", rand::random::<u128>());
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(
        admin_url
            .parse::<sqlx::postgres::PgConnectOptions>()
            .unwrap()
            .database(&database),
    )
    .await
    .unwrap();
    postgres::apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    postgres::canonical_apparatus_service(pool.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    postgres::apply_foundation_migration(&pool).await.unwrap();
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(
            postgres::canonical_apparatus_service(pool.clone()),
        )),
    );
    service.upsert_map(serde_json::from_value(json!({
        "id":ORDER,"title":"History","product_code":"HISTORY",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"press","kind":"apparatus","title":"Press","apparatus_id":PRODUCER},
            {"id":"lam","kind":"apparatus","title":"Lam","apparatus_id":DOWNSTREAM},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"press"},{"from":"press","to":"lam"},{"from":"lam","to":"end"}]
    })).unwrap()).await.unwrap();
    assert_history(store.as_ref()).await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
