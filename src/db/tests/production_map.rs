use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::production_map::{
    OrderProgressBatch, OrderProgressBatchStatus, OrderProgressBatchWipStatus,
    ProductionMapDefinition, ProductionMapEdge, ProductionMapError, ProductionMapNode,
    ProductionMapNodeKind, ProductionMapService, ProductionMapStorePort, queue_state,
};
use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_production_map::PostgresProductionMapStore;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_production_maps"]
async fn postgres_production_map_store_persists_maps_sequences_and_queue_states() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_production_maps";
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let test_url = format!("postgres://wikki@127.0.0.1:5432/{db_name}");
    let pool = sqlx::PgPool::connect(&test_url).await.expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new(store.clone());

    let saved = service
        .upsert_map(test_map("zakaz-1001", "1001", "HOT"))
        .await
        .expect("save map");
    assert_eq!(saved.map.id, "zakaz-1001");
    assert_eq!(saved.map.order_number, "1001");
    let node_rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT node_id, kind, title
             FROM mini_production_map_nodes
             WHERE map_id = $1
             ORDER BY node_id",
    )
    .bind("zakaz-1001")
    .fetch_all(&pool)
    .await
    .expect("read mirrored nodes");
    assert_eq!(
        node_rows,
        vec![
            (
                "apparatus".to_string(),
                "apparatus".to_string(),
                "7 ta rangli pechat".to_string(),
            ),
            ("end".to_string(), "end".to_string(), "End".to_string()),
            (
                "start".to_string(),
                "start".to_string(),
                "Start".to_string()
            ),
        ]
    );
    let edge_rows: Vec<(i32, String, String)> = sqlx::query_as(
        "SELECT edge_index, from_node_id, to_node_id
             FROM mini_production_map_edges
             WHERE map_id = $1
             ORDER BY edge_index",
    )
    .bind("zakaz-1001")
    .fetch_all(&pool)
    .await
    .expect("read mirrored edges");
    assert_eq!(
        edge_rows,
        vec![
            (0, "start".to_string(), "apparatus".to_string()),
            (1, "apparatus".to_string(), "end".to_string()),
        ]
    );

    let duplicate = service
        .upsert_map(test_map("zakaz-1002", "1001", "OTHER"))
        .await;
    assert_eq!(duplicate, Err(ProductionMapError::DuplicateOrderNumber));

    service
        .set_apparatus_sequence(
            "7 ta rangli pechat",
            vec!["zakaz-1001".to_string(), " ".to_string()],
        )
        .await
        .expect("save sequence");
    let mut states = BTreeMap::new();
    states.insert("zakaz-1001".to_string(), "in_progress".to_string());
    service
        .apply_apparatus_queue_action(
            "7 ta rangli pechat",
            "zakaz-1001",
            crate::core::production_map::queue_state::ApparatusQueueAction::Complete,
            &["7 ta rangli pechat".to_string()],
            crate::core::production_map::QueueActionActor {
                role: "admin".to_string(),
                ref_: "test".to_string(),
                display_name: "Test Admin".to_string(),
            },
        )
        .await
        .expect_err("cannot complete before state exists through service");

    store
        .put_apparatus_queue_states("7 ta rangli pechat", states)
        .await
        .expect("save queue states");
    let snapshot = service.live_snapshot().await.expect("snapshot");
    assert_eq!(
        snapshot
            .sequences
            .get("7 ta rangli pechat")
            .expect("sequence"),
        &vec!["zakaz-1001".to_string()]
    );
    assert_eq!(
        snapshot
            .queue_states
            .get("7 ta rangli pechat")
            .and_then(|items| items.get("zakaz-1001")),
        Some(&"in_progress".to_string())
    );

    service
        .restore_map(None, "zakaz-1001")
        .await
        .expect("delete map");
    assert!(service.maps().await.expect("maps").is_empty());

    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("cleanup test db");
    admin_pool.close().await;
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_wip_batches"]
async fn postgres_wip_batches_match_apparatus_instance_suffixes() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_wip_batches";
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let test_url = format!("postgres://wikki@127.0.0.1:5432/{db_name}");
    let pool = sqlx::PgPool::connect(&test_url).await.expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new(store.clone());
    store
        .put_order_progress_batch(wip_batch("Laminatsiya - A"))
        .await
        .expect("put wip batch");

    let batches = service
        .wip_progress_batches(
            "Laminatsiya",
            "",
            "",
            Some(OrderProgressBatchWipStatus::Waiting),
            "",
            100,
        )
        .await
        .expect("wip batches");

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].batch_id, "batch-wip-suffix");
    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("cleanup test db");
    admin_pool.close().await;
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_wip_batches_paging"]
async fn postgres_wip_batches_scan_past_first_page_for_matching_apparatus() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_wip_batches_paging";
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let test_url = format!("postgres://wikki@127.0.0.1:5432/{db_name}");
    let pool = sqlx::PgPool::connect(&test_url).await.expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new(store.clone());
    store
        .put_order_progress_batch(wip_batch("Laminatsiya - A"))
        .await
        .expect("put target wip batch");
    sqlx::query(
        "UPDATE mini_progress_batches
         SET updated_at = now() - interval '1 day'
         WHERE batch_id = 'batch-wip-suffix'",
    )
    .execute(&pool)
    .await
    .expect("age target batch");
    sqlx::query(
        "INSERT INTO mini_progress_batches (
            batch_id, session_id, apparatus, order_id, action, status,
            produced_qty, uom, qr_payload, label_item_code, label_item_name,
            executor_name, worker_role, worker_ref, worker_display_name,
            wip_status, current_apparatus, current_apparatus_key, current_location, payload_json,
            created_at, updated_at
         )
         SELECT 'noise-batch-' || item,
                'noise-session-' || item,
                'Paket aparat',
                'noise-order-' || item,
                'pause',
                'paused',
                1,
                'kg',
                'noise-qr-' || item,
                'noise-order-' || item,
                'Noise item',
                'Worker',
                'aparatchi',
                'worker-noise',
                'Worker Noise',
                'waiting',
                'Paket aparat',
                'paket aparat',
                'Paket aparat',
                '{}'::jsonb,
                now(),
                now() + (item || ' seconds')::interval
         FROM generate_series(1, 5001) AS item",
    )
    .execute(&pool)
    .await
    .expect("insert noise batches");

    let batches = service
        .wip_progress_batches(
            "Laminatsiya",
            "",
            "",
            Some(OrderProgressBatchWipStatus::Waiting),
            "",
            100,
        )
        .await
        .expect("wip batches");

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].batch_id, "batch-wip-suffix");
    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("cleanup test db");
    admin_pool.close().await;
}

fn test_map(id: &str, order_number: &str, product_code: &str) -> ProductionMapDefinition {
    ProductionMapDefinition {
        id: id.to_string(),
        product_code: product_code.to_string(),
        title: "Test map".to_string(),
        code: order_number.to_string(),
        order_number: order_number.to_string(),
        roll_count: Some(7.0),
        width_mm: Some(650.0),
        order_kg: None,
        base_length: None,
        nodes: vec![
            test_node("start", ProductionMapNodeKind::Start, "Start", 0.0),
            test_node(
                "apparatus",
                ProductionMapNodeKind::Apparatus,
                "7 ta rangli pechat",
                120.0,
            ),
            test_node("end", ProductionMapNodeKind::End, "End", 240.0),
        ],
        edges: vec![
            ProductionMapEdge {
                from: "start".to_string(),
                to: "apparatus".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "apparatus".to_string(),
                to: "end".to_string(),
                branch: String::new(),
            },
        ],
    }
}

fn test_node(id: &str, kind: ProductionMapNodeKind, title: &str, y: f64) -> ProductionMapNode {
    ProductionMapNode {
        id: id.to_string(),
        kind,
        title: title.to_string(),
        formula: None,
        role_code: String::new(),
        item_code: String::new(),
        qty_formula: String::new(),
        from_location: String::new(),
        to_location: String::new(),
        alternative_group_id: String::new(),
        alternative_group_label: String::new(),
        alternative_assigned_title: String::new(),
        rezka_kadr_count: None,
        rezka_label_length: None,
        x: 0.0,
        y,
    }
}

fn wip_batch(current_apparatus: &str) -> OrderProgressBatch {
    OrderProgressBatch {
        batch_id: "batch-wip-suffix".to_string(),
        session_id: "session-wip-suffix".to_string(),
        apparatus: "Laminatsiya - A".to_string(),
        order_id: "order-wip-suffix".to_string(),
        action: queue_state::ApparatusQueueAction::Pause,
        status: OrderProgressBatchStatus::Paused,
        produced_qty: 100.0,
        uom: "kg".to_string(),
        qr_payload: "qr-wip-suffix".to_string(),
        label_item_code: "order-wip-suffix".to_string(),
        label_item_name: "WIP suffix".to_string(),
        executor_name: "Worker".to_string(),
        worker_role: "aparatchi".to_string(),
        worker_ref: "worker-wip-suffix".to_string(),
        worker_display_name: "Worker WIP".to_string(),
        wip_status: OrderProgressBatchWipStatus::Waiting,
        current_apparatus: current_apparatus.to_string(),
        current_apparatus_key: queue_state::apparatus_search_key(current_apparatus),
        current_location: current_apparatus.to_string(),
        next_apparatus: String::new(),
        parent_batch_id: String::new(),
        used_by_session_id: String::new(),
        used_by_apparatus: String::new(),
        processed_by_session_id: String::new(),
        processed_by_apparatus: String::new(),
        return_ink_kg: None,
        lamination_print_leftover_rolls: None,
        lamination_film_leftover_rolls: None,
        rezka_bosma_waste: None,
        rezka_lamination_waste: None,
        rezka_edge_waste: None,
        total_waste: None,
        finished_goods_kg: None,
        finished_goods_meter: None,
        description: String::new(),
        payload_json: serde_json::json!({}),
    }
}
