use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::production_map::{
    CompletedQueueOrderStatus, OrderProgressBatch, OrderProgressBatchStatus,
    OrderProgressBatchStatusDetail, OrderProgressBatchWipStatus, ProductionMapDefinition,
    ProductionMapEdge, ProductionMapError, ProductionMapNode, ProductionMapNodeKind,
    ProductionMapService, ProductionMapStorePort, WipProgressBatchQuery, queue_state,
};
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_production_map::PostgresProductionMapStore;

use super::seed_standard_canonical_apparatus;

#[tokio::test]
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

    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, db_name))
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    seed_standard_canonical_apparatus(&pool).await;
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new_for_test(store.clone());

    let saved = service
        .upsert_map(test_map("zakaz-1001", "1001", "HOT"))
        .await
        .expect("save map");
    assert_eq!(saved.map.id, "zakaz-1001");
    assert_eq!(saved.map.order_number, "1001");
    let released_lifecycle: String =
        sqlx::query_scalar("SELECT lifecycle_status FROM mini_production_maps WHERE id = $1")
            .bind("zakaz-1001")
            .fetch_one(&pool)
            .await
            .expect("released production-order lifecycle");
    assert_eq!(released_lifecycle, "released");
    sqlx::query(
        "UPDATE mini_production_maps
         SET map_json = jsonb_set(map_json, '{roll_count}', '7.0'::jsonb)
         WHERE id = $1",
    )
    .bind("zakaz-1001")
    .execute(&pool)
    .await
    .expect("write legacy decimal roll count");
    let legacy_maps = service
        .maps()
        .await
        .expect("list legacy decimal roll count");
    assert_eq!(legacy_maps.len(), 1);
    assert_eq!(legacy_maps[0].map.roll_count, Some(7));
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
                "apparatus:default:bosma_7".to_string(),
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
            "apparatus:default:bosma_7",
            vec!["zakaz-1001".to_string(), " ".to_string()],
        )
        .await
        .expect("save sequence");
    let mut states = BTreeMap::new();
    states.insert("zakaz-1001".to_string(), "in_progress".to_string());
    service
        .apply_apparatus_queue_action(
            "apparatus:default:bosma_7",
            "zakaz-1001",
            crate::core::production_map::queue_state::ApparatusQueueAction::Complete,
            &["apparatus:default:bosma_7".to_string()],
            crate::core::production_map::QueueActionActor {
                role: "admin".to_string(),
                ref_: "test".to_string(),
                display_name: "Test Admin".to_string(),
            },
        )
        .await
        .expect_err("cannot complete before state exists through service");

    store
        .put_apparatus_queue_states("apparatus:default:bosma_7", states)
        .await
        .expect("save queue states");
    let in_progress_lifecycle: String =
        sqlx::query_scalar("SELECT lifecycle_status FROM mini_production_maps WHERE id = $1")
            .bind("zakaz-1001")
            .fetch_one(&pool)
            .await
            .expect("in-progress production-order lifecycle");
    assert_eq!(in_progress_lifecycle, "in_progress");
    let in_progress_operational_status: String =
        sqlx::query_scalar("SELECT operational_status FROM mini_production_maps WHERE id = $1")
            .bind("zakaz-1001")
            .fetch_one(&pool)
            .await
            .expect("in-progress operational projection");
    assert_eq!(in_progress_operational_status, "in_progress");
    store
        .put_apparatus_queue_states(
            "apparatus:default:bosma_7",
            BTreeMap::from([("zakaz-1001".to_string(), "pending".to_string())]),
        )
        .await
        .expect("requeue operation as pending");
    let lifecycle_after_requeue: String =
        sqlx::query_scalar("SELECT lifecycle_status FROM mini_production_maps WHERE id = $1")
            .bind("zakaz-1001")
            .fetch_one(&pool)
            .await
            .expect("monotonic production-order lifecycle");
    assert_eq!(lifecycle_after_requeue, "in_progress");
    let operational_status_after_requeue: String =
        sqlx::query_scalar("SELECT operational_status FROM mini_production_maps WHERE id = $1")
            .bind("zakaz-1001")
            .fetch_one(&pool)
            .await
            .expect("ready operational projection after requeue");
    assert_eq!(operational_status_after_requeue, "ready");
    store
        .put_apparatus_queue_states(
            "apparatus:default:bosma_7",
            BTreeMap::from([("zakaz-1001".to_string(), "in_progress".to_string())]),
        )
        .await
        .expect("restore operation state for snapshot assertion");
    let snapshot = service.live_snapshot().await.expect("snapshot");
    assert_eq!(
        snapshot
            .sequences
            .get("apparatus:default:bosma_7")
            .expect("sequence"),
        &vec!["zakaz-1001".to_string()]
    );
    assert_eq!(
        snapshot
            .queue_states
            .get("apparatus:default:bosma_7")
            .and_then(|items| items.get("zakaz-1001")),
        Some(&"in_progress".to_string())
    );

    store
        .put_apparatus_queue_states(
            "apparatus:default:bosma_7",
            BTreeMap::from([("zakaz-1001".to_string(), "completed".to_string())]),
        )
        .await
        .expect("complete only required operation");
    let completed_lifecycle: String =
        sqlx::query_scalar("SELECT lifecycle_status FROM mini_production_maps WHERE id = $1")
            .bind("zakaz-1001")
            .fetch_one(&pool)
            .await
            .expect("production-completed lifecycle");
    assert_eq!(completed_lifecycle, "production_completed");
    let completed_operational_status: String =
        sqlx::query_scalar("SELECT operational_status FROM mini_production_maps WHERE id = $1")
            .bind("zakaz-1001")
            .fetch_one(&pool)
            .await
            .expect("completed operational projection");
    assert_eq!(completed_operational_status, "completed");
    let lifecycle_event_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mini_production_order_lifecycle_events WHERE order_id = $1",
    )
    .bind("zakaz-1001")
    .fetch_one(&pool)
    .await
    .expect("lifecycle transition events");
    assert_eq!(lifecycle_event_count, 2);

    sqlx::query(
        "INSERT INTO mini_order_run_sessions
            (session_id, apparatus, canonical_apparatus_id, order_id, status)
         VALUES ($1, $2, $2, $3, 'active')",
    )
    .bind("session-delete-protection")
    .bind("apparatus:default:bosma_7")
    .bind("zakaz-1001")
    .execute(&pool)
    .await
    .expect("insert protected run session");
    service
        .restore_map(None, "zakaz-1001")
        .await
        .expect_err("run session must protect production map deletion");
    assert!(service.map("zakaz-1001").await.expect("map").is_some());
    let preserved_queue_state_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_queue_states WHERE order_id = $1")
            .bind("zakaz-1001")
            .fetch_one(&pool)
            .await
            .expect("count preserved queue states");
    assert_eq!(preserved_queue_state_count, 1);
    sqlx::query("DELETE FROM mini_order_run_sessions WHERE session_id = $1")
        .bind("session-delete-protection")
        .execute(&pool)
        .await
        .expect("remove protected run session");

    let audit = service
        .audit_production_workflow()
        .await
        .expect("audit production workflow");
    assert!(
        audit.ok,
        "unexpected workflow audit violations: {:?}",
        audit.violations
    );

    service
        .restore_map(None, "zakaz-1001")
        .await
        .expect("delete map");
    assert!(service.maps().await.expect("maps").is_empty());
    let queue_state_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_queue_states WHERE order_id = $1")
            .bind("zakaz-1001")
            .fetch_one(&pool)
            .await
            .expect("count deleted queue states");
    let sequence_order_ids: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT order_ids FROM mini_queue_sequences WHERE apparatus = $1")
            .bind("apparatus:default:bosma_7")
            .fetch_optional(&pool)
            .await
            .expect("read cleaned sequence");
    assert_eq!(queue_state_count, 0);
    assert!(sequence_order_ids.is_none());

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
async fn postgres_completed_queue_history_returns_actor_stage_completion() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_completed_queue_history";
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

    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, db_name))
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    seed_standard_canonical_apparatus(&pool).await;
    let store = PostgresProductionMapStore::new(pool.clone());
    let service = ProductionMapService::new_for_test(Arc::new(store.clone()));
    service
        .upsert_map(test_map("zakaz-history-1", "9901", "HISTORY"))
        .await
        .expect("save map");

    sqlx::query(
        "INSERT INTO mini_queue_action_events
            (event_id, apparatus, canonical_apparatus_id, order_id, action,
             from_state, to_state, policy, actor_role, actor_ref,
             actor_display_name, assigned_apparatus, payload_json, created_at)
         VALUES
            ($1, $2, $2, $3, 'complete', 'in_progress', 'completed',
             'strict_sequence', 'aparatchi', $4, 'History Worker',
             jsonb_build_array($2::text), '{}'::jsonb,
             to_timestamp(1787402980))",
    )
    .bind("queue-history-complete-1")
    .bind("apparatus:default:bosma_7")
    .bind("zakaz-history-1")
    .bind("worker-history-1")
    .execute(&pool)
    .await
    .expect("insert completed queue event");

    let history = store
        .completed_queue_orders_for_actor("worker-history-1", 10)
        .await
        .expect("load actor stage completion history");

    assert_eq!(history.len(), 1);
    assert_eq!(history[0].order_id, "zakaz-history-1");
    assert_eq!(history[0].apparatus, "apparatus:default:bosma_7");
    assert_eq!(history[0].status, CompletedQueueOrderStatus::Completed);
    assert_eq!(history[0].completed_at_unix, 1_787_402_980);

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
async fn postgres_wip_batches_match_exact_canonical_apparatus_id() {
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

    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, db_name))
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    seed_standard_canonical_apparatus(&pool).await;
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new_for_test(store.clone());
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ('order-wip-suffix', 'WIP-SUFFIX', 'WIP suffix', '{}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("seed WIP production map");
    store
        .put_order_progress_batch(wip_batch("apparatus:default:asset-007"))
        .await
        .expect("put wip batch");

    let batches = service
        .wip_progress_batches(WipProgressBatchQuery::new(
            "apparatus:default:asset-007",
            "",
            "",
            Some(OrderProgressBatchWipStatus::Waiting),
            false,
            "",
            100,
        ))
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

    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, db_name))
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    seed_standard_canonical_apparatus(&pool).await;
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new_for_test(store.clone());
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ('order-wip-suffix', 'WIP-SUFFIX', 'WIP suffix', '{}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("seed target WIP production map");
    store
        .put_order_progress_batch(wip_batch("apparatus:default:asset-007"))
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
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         SELECT 'noise-order-' || item,
                'NOISE-' || item,
                'Noise ' || item,
                '{}'::jsonb
         FROM generate_series(1, 5001) AS item",
    )
    .execute(&pool)
    .await
    .expect("insert noise production maps");
    sqlx::query(
        "INSERT INTO mini_progress_batches (
            batch_id, session_id, apparatus, canonical_apparatus_id,
            order_id, action, status,
            produced_qty, uom, qr_payload, label_item_code, label_item_name,
            executor_name, worker_role, worker_ref, worker_display_name,
            wip_status, current_apparatus, canonical_current_apparatus_id,
            current_apparatus_key, current_location,
            next_apparatus, canonical_next_apparatus_id, payload_json,
            created_at, updated_at
         )
         SELECT 'noise-batch-' || item,
                'noise-session-' || item,
                'apparatus:default:paket',
                'apparatus:default:paket',
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
                'apparatus:default:paket',
                'apparatus:default:paket',
                'apparatus:default:paket',
                'apparatus:default:paket',
                'apparatus:default:bosma_7',
                'apparatus:default:bosma_7',
                '{}'::jsonb,
                now(),
                now() + (item || ' seconds')::interval
         FROM generate_series(1, 5001) AS item",
    )
    .execute(&pool)
    .await
    .expect("insert noise batches");

    let batches = service
        .wip_progress_batches(WipProgressBatchQuery::new(
            "apparatus:default:asset-007",
            "",
            "",
            Some(OrderProgressBatchWipStatus::Waiting),
            false,
            "",
            100,
        ))
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
        customer_name: String::new(),
        roll_count: Some(7),
        width_mm: Some(650.0),
        order_kg: None,
        base_length: None,
        nodes: vec![
            test_node("start", ProductionMapNodeKind::Start, "Start", 0.0),
            test_node(
                "apparatus",
                ProductionMapNodeKind::Apparatus,
                "apparatus:default:bosma_7",
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
    let is_apparatus = kind == ProductionMapNodeKind::Apparatus;
    ProductionMapNode {
        id: id.to_string(),
        kind,
        title: title.to_string(),
        apparatus_id: if is_apparatus {
            title.to_string()
        } else {
            String::new()
        },
        formula: None,
        role_code: String::new(),
        item_code: String::new(),
        qty_formula: String::new(),
        from_location: String::new(),
        to_location: String::new(),
        alternative_group_id: String::new(),
        alternative_group_label: String::new(),
        alternative_assigned_title: String::new(),
        alternative_assigned_apparatus_id: String::new(),
        rezka_kadr_count: None,
        rezka_label_length: None,
        x: 0.0,
        y,
    }
}

fn wip_batch(current_apparatus: &str) -> OrderProgressBatch {
    OrderProgressBatch {
        batch_id: "batch-wip-suffix".to_string(),
        revision: 1,
        session_id: "session-wip-suffix".to_string(),
        started_at_unix: 0,
        completed_at_unix: 0,
        apparatus: "apparatus:default:asset-007".to_string(),
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
        status_detail: OrderProgressBatchStatusDetail::default(),
        current_apparatus: current_apparatus.to_string(),
        current_apparatus_key: queue_state::apparatus_search_key(current_apparatus),
        current_location: current_apparatus.to_string(),
        next_apparatus: "apparatus:default:paket".to_string(),
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
        bobina_kg: None,
        finished_goods_meter: None,
        diameter: None,
        description: String::new(),
        payload_json: serde_json::json!({}),
    }
}
