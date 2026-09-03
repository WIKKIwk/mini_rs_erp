use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::production_map::{
    CompletedQueueOrderStatus, OrderProgressBatch, OrderProgressBatchStatus,
    OrderProgressBatchStatusDetail, OrderProgressBatchWipStatus, OrderRunSession, OrderRunStatus,
    ProductionMapDefinition, ProductionMapEdge, ProductionMapError, ProductionMapNode,
    ProductionMapNodeKind, ProductionMapService, ProductionMapStorePort, WipProgressBatchQuery,
    queue_state,
};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through, postgres_test_database_options,
};
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
    let wip_map = test_map("order-wip-suffix", "9001", "WIP-SUFFIX");
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ('order-wip-suffix', 'WIP-SUFFIX', 'WIP suffix', $1)",
    )
    .bind(serde_json::to_value(&wip_map).expect("valid wip test map"))
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
    let wip_map = test_map("order-wip-suffix", "9001", "WIP-SUFFIX");
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ('order-wip-suffix', 'WIP-SUFFIX', 'WIP suffix', $1)",
    )
    .bind(serde_json::to_value(&wip_map).expect("valid wip test map"))
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
            current_location,
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
        rezka_frame_groups: Vec::new(),
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

#[tokio::test]
async fn postgres_order_run_session_stage_identity_real_cutover_and_invariants() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://superuser@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_order_run_session_stage_identity";
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

    // =========================================================================
    // A. Legacy migration backfill
    // =========================================================================
    // Apply migrations 1..=87 (pre-0088 schema)
    apply_postgres_migrations_through(&pool, 87)
        .await
        .expect("apply migrations up to 0087");
    seed_standard_canonical_apparatus(&pool).await;

    // In pre-0088, column stage_node_id does NOT exist on mini_order_run_sessions
    // First, seed an order so foreign keys are satisfied
    let test_order_id = "zakaz-legacy-backfill-01";
    let apparatus = "apparatus:default:asset-010";
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ($1, 'TEST', 'Test Map', '{}'::jsonb)",
    )
    .bind(test_order_id)
    .execute(&pool)
    .await
    .expect("seed test map");

    // Insert legacy row where stage_node_id is ONLY inside payload_json
    let legacy_session_id = "session-legacy-stage-01";
    sqlx::query(
        "INSERT INTO mini_order_run_sessions
            (session_id, apparatus, canonical_apparatus_id, order_id, status, payload_json)
         VALUES ($1, $2, $2, $3, 'completed', $4)",
    )
    .bind(legacy_session_id)
    .bind(apparatus)
    .bind(test_order_id)
    .bind(serde_json::json!({
        "stage_node_id": "rezka_first",
        "worker_note": "legacy pre-0088 production run"
    }))
    .execute(&pool)
    .await
    .expect("insert legacy pre-0088 session");

    // Now apply migration 0088
    apply_postgres_migrations_through(&pool, 88)
        .await
        .expect("apply migration 0088");

    // Query DB and verify:
    // 1. stage_node_id column is populated with "rezka_first"
    // 2. payload_json ? 'stage_node_id' is false
    let (migrated_stage, has_payload_key, remaining_payload): (String, bool, serde_json::Value) =
        sqlx::query_as(
            "SELECT stage_node_id, (payload_json ? 'stage_node_id'), payload_json
             FROM mini_order_run_sessions
             WHERE session_id = $1",
        )
        .bind(legacy_session_id)
        .fetch_one(&pool)
        .await
        .expect("fetch migrated session");

    assert_eq!(migrated_stage, "rezka_first");
    assert!(!has_payload_key);
    assert_eq!(
        remaining_payload["worker_note"],
        "legacy pre-0088 production run"
    );
    assert!(remaining_payload.get("stage_node_id").is_none());

    // =========================================================================
    // B. Typed persistence round-trip
    // =========================================================================
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let new_session_id = "session-typed-stage-02";
    let typed_session = OrderRunSession {
        session_id: new_session_id.to_string(),
        apparatus: apparatus.to_string(),
        order_id: test_order_id.to_string(),
        stage_node_id: "rezka_final".to_string(),
        status: OrderRunStatus::Active,
        worker_role: "aparatchi".to_string(),
        worker_ref: "worker-01".to_string(),
        worker_display_name: "Ali".to_string(),
        started_at_unix: 200,
        updated_at_unix: 250,
        payload_json: serde_json::json!({
            "note": "typed runtime session"
        }),
    };

    // Real store path: put_order_run_session
    store
        .put_order_run_session(typed_session)
        .await
        .expect("put typed order run session");

    // Real store path: active_order_run_session
    let loaded_active = store
        .active_order_run_session(apparatus, test_order_id)
        .await
        .expect("load active session query")
        .expect("session present");
    assert_eq!(loaded_active.session_id, new_session_id);
    assert_eq!(loaded_active.stage_node_id, "rezka_final");
    assert!(loaded_active.payload_json.get("stage_node_id").is_none());
    assert_eq!(loaded_active.payload_json["note"], "typed runtime session");

    // Real store path: order_run_sessions_for_order
    let all_order_sessions = store
        .order_run_sessions_for_order(test_order_id)
        .await
        .expect("load all sessions for order");
    assert_eq!(all_order_sessions.len(), 2);
    let loaded_second = all_order_sessions
        .iter()
        .find(|s| s.session_id == new_session_id)
        .expect("second session in list");
    assert_eq!(loaded_second.stage_node_id, "rezka_final");
    assert!(loaded_second.payload_json.get("stage_node_id").is_none());

    // =========================================================================
    // C. DB constraints really reject bad writes
    // =========================================================================
    // 1. Untrimmed typed identity is rejected
    let untrimmed_err = sqlx::query(
        "INSERT INTO mini_order_run_sessions
            (session_id, apparatus, canonical_apparatus_id, order_id, stage_node_id, status)
         VALUES ($1, $2, $2, $3, $4, 'completed')",
    )
    .bind("session-bad-untrimmed")
    .bind(apparatus)
    .bind(test_order_id)
    .bind(" rezka_final ")
    .execute(&pool)
    .await
    .expect_err("untrimmed stage_node_id must be rejected by check constraint");

    let db_err = untrimmed_err.as_database_error().expect("db error");
    assert_eq!(
        db_err.constraint(),
        Some("mini_order_run_sessions_stage_node_id_trimmed")
    );

    // 2. Attempting to store {"stage_node_id": "rezka_final"} in payload_json is rejected
    let forbidden_payload_err = sqlx::query(
        "INSERT INTO mini_order_run_sessions
            (session_id, apparatus, canonical_apparatus_id, order_id, stage_node_id, status, payload_json)
         VALUES ($1, $2, $2, $3, $4, 'completed', $5)",
    )
    .bind("session-bad-forbidden-payload")
    .bind(apparatus)
    .bind(test_order_id)
    .bind("rezka_final")
    .bind(serde_json::json!({
        "stage_node_id": "rezka_final"
    }))
    .execute(&pool)
    .await
    .expect_err("stage_node_id in payload_json must be rejected by check constraint");

    let db_err = forbidden_payload_err.as_database_error().expect("db error");
    assert_eq!(
        db_err.constraint(),
        Some("mini_order_run_sessions_stage_payload_forbidden")
    );

    // Cleanup test database
    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
}

#[tokio::test]
async fn postgres_progress_batch_typed_payload_mirrors_real_cutover_and_invariants() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://superuser@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_progress_batch_typed_payload_mirrors";
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

    // 1. Apply migrations 1..=88 (pre-0089 schema)
    apply_postgres_migrations_through(&pool, 88)
        .await
        .expect("apply migrations up to 0088");
    seed_standard_canonical_apparatus(&pool).await;

    let test_order_id = "zakaz-batch-mirrors-01";
    let apparatus = "apparatus:default:asset-010";
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ($1, 'TEST', 'Test Map', '{}'::jsonb)",
    )
    .bind(test_order_id)
    .execute(&pool)
    .await
    .expect("seed test map");

    // Insert legacy pre-0089 batch with all 13 duplicate keys in payload_json + unrelated payload metadata
    let legacy_batch_id = "batch-legacy-mirrors-01";
    let legacy_session_id = "session-legacy-01";
    let legacy_payload = serde_json::json!({
        "status_detail": {"work_status": "in_progress", "wip_status": "waiting", "flow_status": "free_wip"},
        "wip_status": "waiting",
        "current_apparatus": apparatus,
        "current_apparatus_key": apparatus,
        "current_location": "apparatus:default:asset-010 chiqim",
        "next_apparatus": "apparatus:default:asset-011",
        "parent_batch_id": "batch-parent-01",
        "used_by_session_id": "session-used-01",
        "used_by_apparatus": "apparatus:default:asset-011",
        "used_by_order_id": test_order_id,
        "processed_by_session_id": "session-proc-01",
        "processed_by_apparatus": "apparatus:default:asset-011",
        "from_apparatus": apparatus,
        "unrelated_meta": "preserve_this_value",
        "stage_node_id": "rezka_stage_1",
        "contained_kadr_count": 42
    });

    sqlx::query(
        "INSERT INTO mini_progress_batches (
            batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
            action, status, produced_qty, uom, qr_payload, label_item_code, label_item_name,
            wip_status, current_apparatus, current_apparatus_key, current_location, next_apparatus,
            parent_batch_id, used_by_session_id, used_by_apparatus,
            processed_by_session_id, processed_by_apparatus, payload_json
         )
         VALUES (
            $1, $2, $3, $3, $4,
            'pause', 'paused', 100.0, 'm', 'qr:batch-legacy-01', $4, 'Item Test',
            'waiting', $3, $3, 'apparatus:default:asset-010 chiqim', 'apparatus:default:asset-011',
            'batch-parent-01', 'session-used-01', 'apparatus:default:asset-011',
            'session-proc-01', 'apparatus:default:asset-011', $5
         )",
    )
    .bind(legacy_batch_id)
    .bind(legacy_session_id)
    .bind(apparatus)
    .bind(test_order_id)
    .bind(&legacy_payload)
    .execute(&pool)
    .await
    .expect("insert legacy pre-0089 batch");

    // 2. Apply migration 0089
    apply_postgres_migrations_through(&pool, 89)
        .await
        .expect("apply migration 0089");

    // 3. Assertions:
    // a) Typed SQL columns are unchanged and correct
    let (
        wip_status,
        current_apparatus,
        current_location,
        next_apparatus,
        parent_batch_id,
        used_by_session_id,
        used_by_apparatus,
        processed_by_session_id,
        processed_by_apparatus,
        has_forbidden_keys,
        remaining_payload,
    ): (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        bool,
        serde_json::Value,
    ) = sqlx::query_as(
        "SELECT
            wip_status,
            current_apparatus,
            current_location,
            next_apparatus,
            parent_batch_id,
            used_by_session_id,
            used_by_apparatus,
            processed_by_session_id,
            processed_by_apparatus,
            (payload_json ?| array[
                'status_detail', 'wip_status', 'current_apparatus', 'current_apparatus_key',
                'current_location', 'next_apparatus', 'parent_batch_id', 'used_by_session_id',
                'used_by_apparatus', 'used_by_order_id', 'processed_by_session_id',
                'processed_by_apparatus', 'from_apparatus'
            ]::text[]),
            payload_json
         FROM mini_progress_batches
         WHERE batch_id = $1",
    )
    .bind(legacy_batch_id)
    .fetch_one(&pool)
    .await
    .expect("fetch migrated batch");

    assert_eq!(wip_status, "waiting");
    assert_eq!(current_apparatus, apparatus);
    assert_eq!(current_location, "apparatus:default:asset-010 chiqim");
    assert_eq!(next_apparatus, "apparatus:default:asset-011");
    assert_eq!(parent_batch_id, "batch-parent-01");
    assert_eq!(used_by_session_id, "session-used-01");
    assert_eq!(used_by_apparatus, "apparatus:default:asset-011");
    assert_eq!(processed_by_session_id, "session-proc-01");
    assert_eq!(processed_by_apparatus, "apparatus:default:asset-011");

    // b) Duplicate payload keys removed
    assert!(
        !has_forbidden_keys,
        "payload_json must not contain any forbidden mirror keys"
    );

    // c) Unrelated payload metadata preserved
    assert_eq!(remaining_payload["unrelated_meta"], "preserve_this_value");
    assert_eq!(remaining_payload["stage_node_id"], "rezka_stage_1");
    assert_eq!(remaining_payload["contained_kadr_count"], 42);

    // 4. Invariant check: attempting to reintroduce any forbidden typed mirror key into payload_json is rejected by CHECK constraint
    for forbidden_key in [
        "status_detail",
        "wip_status",
        "current_apparatus",
        "current_apparatus_key",
        "current_location",
        "next_apparatus",
        "parent_batch_id",
        "used_by_session_id",
        "used_by_apparatus",
        "used_by_order_id",
        "processed_by_session_id",
        "processed_by_apparatus",
        "from_apparatus",
    ] {
        let bad_batch_id = format!("batch-bad-{forbidden_key}");
        let bad_payload = serde_json::json!({
            forbidden_key: "forbidden_value"
        });

        let insert_err = sqlx::query(
            "INSERT INTO mini_progress_batches (
                batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
                action, status, produced_qty, uom, qr_payload, label_item_code, label_item_name,
                payload_json
             )
             VALUES (
                $1, 'sess-test', $2, $2, $3,
                'pause', 'paused', 10.0, 'm', $4, $3, 'Item Test', $5
             )",
        )
        .bind(&bad_batch_id)
        .bind(apparatus)
        .bind(test_order_id)
        .bind(format!("qr:{bad_batch_id}"))
        .bind(bad_payload)
        .execute(&pool)
        .await
        .expect_err(&format!(
            "inserting {forbidden_key} into payload_json must be rejected"
        ));

        let db_err = insert_err.as_database_error().expect("db error");
        assert_eq!(
            db_err.constraint(),
            Some("mini_progress_batches_wip_typed_payload_forbidden")
        );
    }

    // Cleanup test database
    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
}

#[tokio::test]
async fn postgres_drop_progress_batch_current_apparatus_key_real_migration_and_invariants() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://superuser@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_drop_current_apparatus_key";
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

    // 1. Apply migrations 1..=89 (pre-0090 schema where current_apparatus_key column exists)
    apply_postgres_migrations_through(&pool, 89)
        .await
        .expect("apply migrations up to 0089");
    seed_standard_canonical_apparatus(&pool).await;

    let test_order_id = "zakaz-drop-key-01";
    let apparatus_10 = "apparatus:default:asset-010";
    let apparatus_bosma_8 = "apparatus:default:bosma_8";

    let test_map = serde_json::json!({
        "id": test_order_id,
        "product_code": "TEST",
        "title": "Test Map",
        "nodes": [
            { "id": "start", "kind": "start", "title": "Start" },
            { "id": "cut", "kind": "apparatus", "title": "Rezka", "apparatus_id": apparatus_10 },
            { "id": "pack", "kind": "apparatus", "title": "Paket", "apparatus_id": "apparatus:default:paket" }
        ],
        "edges": [
            { "from": "start", "to": "cut" },
            { "from": "cut", "to": "pack" }
        ]
    });

    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ($1, 'TEST', 'Test Map', $2)",
    )
    .bind(test_order_id)
    .bind(&test_map)
    .execute(&pool)
    .await
    .expect("seed test map");

    // Verify pre-0090: column current_apparatus_key exists
    let col_exists_pre: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_name = 'mini_progress_batches' AND column_name = 'current_apparatus_key'
        )",
    )
    .fetch_one(&pool)
    .await
    .expect("check column pre-0090");
    assert!(
        col_exists_pre,
        "current_apparatus_key column must exist pre-0090"
    );

    // Verify pre-0090: obsolete index exists
    let idx_exists_pre: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM pg_indexes
            WHERE tablename = 'mini_progress_batches' AND indexname = 'idx_mini_progress_batches_wip_status_apparatus_key'
        )",
    )
    .fetch_one(&pool)
    .await
    .expect("check index pre-0090");
    assert!(idx_exists_pre, "obsolete index must exist pre-0090");

    // 2. Seed representative legacy rows:
    // Row 1: Valid canonical current apparatus identity + current_apparatus_key populated
    sqlx::query(
        "INSERT INTO mini_progress_batches (
            batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
            action, status, produced_qty, uom, qr_payload, label_item_code, label_item_name,
            wip_status, current_apparatus, canonical_current_apparatus_id, current_apparatus_key,
            current_location, next_apparatus, canonical_next_apparatus_id, payload_json
         )
         VALUES (
            'batch-canonical-01', 'sess-01', $1, $1, $2,
            'pause', 'paused', 100.0, 'm', 'qr:batch-canonical-01', $2, 'Item 01',
            'waiting', $1, $1, $1,
            'loc-01', 'apparatus:default:paket', 'apparatus:default:paket', '{}'::jsonb
         )",
    )
    .bind(apparatus_10)
    .bind(test_order_id)
    .execute(&pool)
    .await
    .expect("seed row 1");

    // Row 2: Canonical identity is valid, but current_apparatus_key has historical display-style key
    sqlx::query(
        "INSERT INTO mini_progress_batches (
            batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
            action, status, produced_qty, uom, qr_payload, label_item_code, label_item_name,
            wip_status, current_apparatus, canonical_current_apparatus_id, current_apparatus_key,
            current_location, next_apparatus, canonical_next_apparatus_id, payload_json
         )
         VALUES (
            'batch-display-key-02', 'sess-02', $1, $1, $2,
            'pause', 'paused', 200.0, 'm', 'qr:batch-display-key-02', $2, 'Item 02',
            'waiting', $1, $1, '8 ta rangli pechat',
            'loc-02', 'apparatus:default:paket', 'apparatus:default:paket', '{}'::jsonb
         )",
    )
    .bind(apparatus_bosma_8)
    .bind(test_order_id)
    .execute(&pool)
    .await
    .expect("seed row 2");

    // Row 3: canonical_current_apparatus_id is NULL and current_apparatus is empty, but current_apparatus_key has valid canonical apparatus id
    sqlx::query(
        "INSERT INTO mini_progress_batches (
            batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
            action, status, produced_qty, uom, qr_payload, label_item_code, label_item_name,
            wip_status, current_apparatus, canonical_current_apparatus_id, current_apparatus_key,
            current_location, next_apparatus, canonical_next_apparatus_id, payload_json
         )
         VALUES (
            'batch-repaired-key-03', 'sess-03', $1, $1, $2,
            'pause', 'paused', 300.0, 'm', 'qr:batch-repaired-key-03', $2, 'Item 03',
            'waiting', '', NULL, $1,
            'loc-03', 'apparatus:default:paket', 'apparatus:default:paket', '{}'::jsonb
         )",
    )
    .bind(apparatus_10)
    .bind(test_order_id)
    .execute(&pool)
    .await
    .expect("seed row 3");

    // 3. Apply migration 0090
    apply_postgres_migrations_through(&pool, 90)
        .await
        .expect("apply migration 0090");

    // 4. Assertions:
    // a) Column current_apparatus_key is dropped
    let col_exists_post: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_name = 'mini_progress_batches' AND column_name = 'current_apparatus_key'
        )",
    )
    .fetch_one(&pool)
    .await
    .expect("check column post-0090");
    assert!(
        !col_exists_post,
        "current_apparatus_key column must be dropped by 0090"
    );

    // b) Obsolete index is dropped
    let idx_exists_post: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM pg_indexes
            WHERE tablename = 'mini_progress_batches' AND indexname = 'idx_mini_progress_batches_wip_status_apparatus_key'
        )",
    )
    .fetch_one(&pool)
    .await
    .expect("check obsolete index post-0090");
    assert!(!idx_exists_post, "obsolete index must be dropped by 0090");

    // c) New index on canonical current apparatus identity exists
    let new_idx_exists_post: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM pg_indexes
            WHERE tablename = 'mini_progress_batches' AND indexname = 'idx_mini_progress_batches_wip_status_canonical_current_apparatus'
        )",
    )
    .fetch_one(&pool)
    .await
    .expect("check new index post-0090");
    assert!(
        new_idx_exists_post,
        "canonical current apparatus index must exist post-0090"
    );

    // d) Row 3 was repaired by migration 0090
    let (repaired_canonical_id, repaired_current_apparatus): (String, String) = sqlx::query_as(
        "SELECT COALESCE(canonical_current_apparatus_id, ''), current_apparatus
         FROM mini_progress_batches
         WHERE batch_id = 'batch-repaired-key-03'",
    )
    .fetch_one(&pool)
    .await
    .expect("fetch repaired row");
    assert_eq!(repaired_canonical_id, apparatus_10);
    assert_eq!(repaired_current_apparatus, apparatus_10);

    // e) Real Postgres WIP queries return exact canonical apparatus matches
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new_for_test(store.clone());

    // Query WIP for apparatus_10: must return batch-canonical-01 and batch-repaired-key-03
    let batches_10 = service
        .wip_progress_batches(WipProgressBatchQuery::new(
            apparatus_10,
            "",
            "",
            Some(OrderProgressBatchWipStatus::Waiting),
            false,
            test_order_id,
            10,
        ))
        .await
        .expect("wip query for apparatus_10");
    assert_eq!(batches_10.len(), 2);
    let ids_10: Vec<String> = batches_10.into_iter().map(|b| b.batch_id).collect();
    assert!(ids_10.contains(&"batch-canonical-01".to_string()));
    assert!(ids_10.contains(&"batch-repaired-key-03".to_string()));

    // Query WIP for apparatus_bosma_8: must return batch-display-key-02
    let batches_8 = service
        .wip_progress_batches(WipProgressBatchQuery::new(
            apparatus_bosma_8,
            "",
            "",
            Some(OrderProgressBatchWipStatus::Waiting),
            false,
            test_order_id,
            10,
        ))
        .await
        .expect("wip query for apparatus_bosma_8");
    assert_eq!(batches_8.len(), 1);
    assert_eq!(batches_8[0].batch_id, "batch-display-key-02");
    assert_eq!(batches_8[0].current_apparatus, apparatus_bosma_8);

    // Querying with historical display string fails validation because canonical identity is authoritative
    let batches_display = service
        .wip_progress_batches(WipProgressBatchQuery::new(
            "8 ta rangli pechat",
            "",
            "",
            Some(OrderProgressBatchWipStatus::Waiting),
            false,
            test_order_id,
            10,
        ))
        .await;
    assert!(
        batches_display.is_err(),
        "non-canonical apparatus query must fail validation"
    );

    // Cleanup test database
    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
}

#[tokio::test]
async fn postgres_0091_migration_backfill_and_write_side_persistence() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://superuser@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_canonical_0091";
    let admin_pool = match sqlx::PgPool::connect(&admin_url).await {
        Ok(pool) => pool,
        Err(err) => {
            eprintln!("Skipping PostgreSQL test: admin db connect failed ({err})");
            return;
        }
    };

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

    // 1. Apply migrations 1..=90
    apply_postgres_migrations_through(&pool, 90)
        .await
        .expect("apply migrations up to 0090");
    seed_standard_canonical_apparatus(&pool).await;

    // Verify pre-0091: flow_status and stock_status columns do not exist yet
    let flow_col_pre: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_name = 'mini_production_maps' AND column_name = 'flow_status'
        )",
    )
    .fetch_one(&pool)
    .await
    .expect("check flow_status column pre-0091");
    assert!(!flow_col_pre, "flow_status must not exist pre-0091");

    // 2. Seed legacy states representing old semantics
    let order_free_wip = "zakaz-legacy-free-wip";
    let order_accepted_stock = "zakaz-legacy-accepted-stock";
    let order_waiting_next = "zakaz-legacy-waiting-next";
    let order_in_progress = "zakaz-legacy-in-progress";

    let apparatus_05 = "apparatus:default:bosma_7";
    let apparatus_07 = "apparatus:default:asset-007";

    for (order_id, op_status) in [
        (order_free_wip, "completed"),
        (order_accepted_stock, "completed"),
        (order_waiting_next, "waiting_next_stage"),
        (order_in_progress, "in_progress"),
    ] {
        let legacy_map = test_map(order_id, "1001", "HOT");
        sqlx::query(
            "INSERT INTO mini_production_maps (
                id, product_code, title, map_json, lifecycle_status, lifecycle_version, operational_status
             ) VALUES (
                $1, 'TEST', 'Test Order', $2, 'released', 1, $3
             )",
        )
        .bind(order_id)
        .bind(serde_json::to_value(&legacy_map).unwrap())
        .bind(op_status)
        .execute(&pool)
        .await
        .expect("seed legacy map");
    }

    // Seed batch for order_free_wip: finished goods output waiting (no next apparatus)
    sqlx::query(
        "INSERT INTO mini_progress_batches (
            batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
            action, status, produced_qty, uom, qr_payload, label_item_code, label_item_name,
            wip_status, current_apparatus, canonical_current_apparatus_id,
            current_location, next_apparatus, canonical_next_apparatus_id, payload_json
         ) VALUES (
            'batch-fg-waiting', 'sess-01', $1, $1, $2,
            'complete', 'completed', 100.0, 'm', 'qr:batch-fg-waiting', 'ITEM-01', 'Item 01',
            'waiting', $1, $1,
            'loc-01', '', NULL, '{}'::jsonb
         )",
    )
    .bind(apparatus_05)
    .bind(order_free_wip)
    .execute(&pool)
    .await
    .expect("seed free wip batch");

    // Seed batch for order_accepted_stock: processed by warehouse
    sqlx::query(
        "INSERT INTO mini_progress_batches (
            batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
            action, status, produced_qty, uom, qr_payload, label_item_code, label_item_name,
            wip_status, current_apparatus, canonical_current_apparatus_id,
            current_location, next_apparatus, canonical_next_apparatus_id, processed_by_apparatus, payload_json
         ) VALUES (
            'batch-fg-accepted', 'sess-02', $1, $1, $2,
            'complete', 'completed', 50.0, 'm', 'qr:batch-fg-accepted', 'ITEM-01', 'Item 01',
            'processed', $1, $1,
            'loc-01', '', NULL, 'warehouse:Tayyor mahsulot ombori', '{}'::jsonb
         )",
    )
    .bind(apparatus_05)
    .bind(order_accepted_stock)
    .execute(&pool)
    .await
    .expect("seed accepted batch");

    // Seed batch for order_waiting_next: waiting for next stage
    sqlx::query(
        "INSERT INTO mini_progress_batches (
            batch_id, session_id, apparatus, canonical_apparatus_id, order_id,
            action, status, produced_qty, uom, qr_payload, label_item_code, label_item_name,
            wip_status, current_apparatus, canonical_current_apparatus_id,
            current_location, next_apparatus, canonical_next_apparatus_id, payload_json
         ) VALUES (
            'batch-wip-next', 'sess-03', $1, $1, $2,
            'complete', 'completed', 75.0, 'm', 'qr:batch-wip-next', 'ITEM-01', 'Item 01',
            'waiting', $1, $1,
            'loc-01', $3, $3, '{}'::jsonb
         )",
    )
    .bind(apparatus_05)
    .bind(order_waiting_next)
    .bind(apparatus_07)
    .execute(&pool)
    .await
    .expect("seed waiting next stage batch");

    // 3. Apply migration 0091
    apply_postgres_migrations_through(&pool, 91)
        .await
        .expect("apply migration 0091");

    // 4. Assertions on Migration Backfill:
    // a) Columns exist
    let flow_col_post: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_name = 'mini_production_maps' AND column_name = 'flow_status'
        )",
    )
    .fetch_one(&pool)
    .await
    .expect("check flow_status post-0091");
    assert!(flow_col_post, "flow_status column must exist post-0091");

    // b) Backfilled values match exact df6b50f semantics
    let (free_flow, free_stock): (String, String) =
        sqlx::query_as("SELECT flow_status, stock_status FROM mini_production_maps WHERE id = $1")
            .bind(order_free_wip)
            .fetch_one(&pool)
            .await
            .expect("fetch free_wip map row");
    assert_eq!(
        free_flow, "free_wip",
        "order with FG waiting must backfill to free_wip"
    );
    assert_eq!(
        free_stock, "",
        "order with FG waiting must have empty stock_status"
    );

    let (accepted_flow, accepted_stock): (String, String) =
        sqlx::query_as("SELECT flow_status, stock_status FROM mini_production_maps WHERE id = $1")
            .bind(order_accepted_stock)
            .fetch_one(&pool)
            .await
            .expect("fetch accepted map row");
    assert_eq!(
        accepted_flow, "accepted_to_stock",
        "order with warehouse accepted FG must backfill to accepted_to_stock"
    );
    assert_eq!(
        accepted_stock, "accepted",
        "order with warehouse accepted FG must backfill to accepted"
    );

    let (waiting_flow, waiting_stock): (String, String) =
        sqlx::query_as("SELECT flow_status, stock_status FROM mini_production_maps WHERE id = $1")
            .bind(order_waiting_next)
            .fetch_one(&pool)
            .await
            .expect("fetch waiting next stage map row");
    assert_eq!(
        waiting_flow, "waiting_next_stage",
        "order waiting next stage must preserve waiting_next_stage"
    );
    assert_eq!(waiting_stock, "");

    let (in_progress_flow, in_progress_stock): (String, String) =
        sqlx::query_as("SELECT flow_status, stock_status FROM mini_production_maps WHERE id = $1")
            .bind(order_in_progress)
            .fetch_one(&pool)
            .await
            .expect("fetch in_progress map row");
    assert_eq!(in_progress_flow, "in_progress");
    assert_eq!(in_progress_stock, "");

    // 5. Test Live Service Write Boundaries:
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new_for_test(store.clone());

    let live_order_id = "zakaz-live-flow-test";
    let mut live_map = test_map(live_order_id, "1002", "HOT");
    for node in &mut live_map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus {
            node.title = "apparatus:default:paket".to_string();
            node.apparatus_id = "apparatus:default:paket".to_string();
        }
    }
    service.upsert_map(live_map).await.expect("upsert live map");
    store
        .put_apparatus_sequence("apparatus:default:paket", vec![live_order_id.to_string()])
        .await
        .expect("save apparatus sequence");

    let worker_actor = crate::core::production_map::QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "worker-01".to_string(),
        display_name: "Worker One".to_string(),
    };
    let assigned = vec!["apparatus:default:paket".to_string()];

    // A. Start action -> in_progress
    service
        .apply_apparatus_queue_action_with_progress(
            "apparatus:default:paket",
            live_order_id,
            queue_state::ApparatusQueueAction::Start,
            &assigned,
            worker_actor.clone(),
            crate::core::production_map::QueueProgressInput::default(),
        )
        .await
        .expect("start queue action");

    // Directly query mini_production_maps table to verify WRITE-SIDE PERSISTENCE
    let (db_op, db_flow, db_stock): (String, String, String) = sqlx::query_as(
        "SELECT operational_status, flow_status, stock_status FROM mini_production_maps WHERE id = $1",
    )
    .bind(live_order_id)
    .fetch_one(&pool)
    .await
    .expect("query mini_production_maps after start");
    assert_eq!(
        db_op, "in_progress",
        "DB operational_status must be persisted as in_progress"
    );
    assert_eq!(
        db_flow, "in_progress",
        "DB flow_status must be persisted as in_progress"
    );
    assert_eq!(db_stock, "", "DB stock_status must be empty");

    // B. Pause action producing finished goods output -> flow_status = free_wip
    let paused_result = service
        .apply_apparatus_queue_action_with_progress(
            "apparatus:default:paket",
            live_order_id,
            queue_state::ApparatusQueueAction::Pause,
            &assigned,
            worker_actor.clone(),
            crate::core::production_map::QueueProgressInput {
                produced_qty: Some(42.0),
                uom: "kg".to_string(),
                ..crate::core::production_map::QueueProgressInput::default()
            },
        )
        .await
        .expect("pause queue action");

    let paused_batch = paused_result.progress_batch.expect("pause produced batch");

    // Directly query mini_production_maps table: must be operational=paused, flow=free_wip
    let (db_op_paused, db_flow_paused, db_stock_paused): (String, String, String) = sqlx::query_as(
        "SELECT operational_status, flow_status, stock_status FROM mini_production_maps WHERE id = $1",
    )
    .bind(live_order_id)
    .fetch_one(&pool)
    .await
    .expect("query mini_production_maps after pause");
    assert_eq!(db_op_paused, "paused");
    assert_eq!(
        db_flow_paused, "free_wip",
        "DB flow_status must be persisted as free_wip"
    );
    assert_eq!(db_stock_paused, "");

    // C. Warehouse receipt -> flow_status = accepted_to_stock, stock_status = accepted
    let warehouse_actor = crate::core::production_map::QueueActionActor {
        role: "werka".to_string(),
        ref_: "warehouse-worker".to_string(),
        display_name: "Warehouse Worker".to_string(),
    };
    service
        .receive_finished_goods(
            &paused_batch.batch_id,
            &paused_batch.qr_payload,
            "Tayyor mahsulot ombori",
            warehouse_actor,
        )
        .await
        .expect("receive finished goods");

    // Directly query mini_production_maps table: MUST BE flow=accepted_to_stock, stock=accepted
    let (db_op_rcv, db_flow_rcv, db_stock_rcv): (String, String, String) = sqlx::query_as(
        "SELECT operational_status, flow_status, stock_status FROM mini_production_maps WHERE id = $1",
    )
    .bind(live_order_id)
    .fetch_one(&pool)
    .await
    .expect("query mini_production_maps after warehouse receipt");
    assert_eq!(db_op_rcv, "paused");
    assert_eq!(
        db_flow_rcv, "accepted_to_stock",
        "DB flow_status must be accepted_to_stock"
    );
    assert_eq!(db_stock_rcv, "accepted", "DB stock_status must be accepted");

    // 6. Read Path Verification:
    // load_production_order_lifecycles directly reads persisted columns without WIP join
    let loaded = crate::db::postgres_production_map::load_production_order_lifecycles(
        &pool,
        &[live_order_id.to_string()],
    )
    .await
    .expect("load lifecycles");
    let record = loaded.get(live_order_id).expect("found lifecycle record");
    assert_eq!(record.flow_status, "accepted_to_stock");
    assert_eq!(record.stock_status, "accepted");

    // order_status_detail API representation must match
    let status_detail = service
        .order_status_detail(live_order_id)
        .await
        .expect("order status detail");
    assert_eq!(status_detail.flow_status, "accepted_to_stock");
    assert_eq!(status_detail.stock_status, "accepted");

    // Cleanup test database
    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
}

#[tokio::test]
async fn postgres_invalid_map_json_fails_closed_and_rolls_back_batch_write() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://superuser@127.0.0.1:5432/postgres".to_string());
    let db_name = format!(
        "mini_rs_erp_test_lifecycle_invalid_map_{}",
        std::process::id()
    );
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

    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migrations");
    seed_standard_canonical_apparatus(&pool).await;

    // Seed a valid order map row that a status-affecting write can target.
    let order_id = "order-invalid-map";
    let map = test_map(order_id, "9009", "INVALID");
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(order_id)
    .bind(&map.product_code)
    .bind(&map.title)
    .bind(serde_json::to_value(&map).expect("valid test map json"))
    .execute(&pool)
    .await
    .expect("seed order map");

    let apparatus = "apparatus:default:asset-010";
    let batch_id = "batch-invalid-map-1";
    let before: (String, String, String, String, i64, i64) = sqlx::query_as(
        "SELECT lifecycle_status, operational_status, flow_status, stock_status,
                lifecycle_version, (SELECT COUNT(*) FROM mini_progress_batches WHERE batch_id = $2)
         FROM mini_production_maps WHERE id = $1",
    )
    .bind(order_id)
    .bind(batch_id)
    .fetch_one(&pool)
    .await
    .expect("before projection");
    assert_eq!(before.5, 0);

    // Corrupt the authoritative map document directly in PostgreSQL.
    sqlx::query("UPDATE mini_production_maps SET map_json = '{\"broken\": true}' WHERE id = $1")
        .bind(order_id)
        .execute(&pool)
        .await
        .expect("corrupt map json");

    // A real status-affecting store write: batch insert + lifecycle refresh
    // in one transaction. It must fail closed, not pretend success.
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let write_result = store
        .put_order_progress_batch(OrderProgressBatch {
            batch_id: batch_id.to_string(),
            revision: 1,
            session_id: "session-invalid-map-1".to_string(),
            started_at_unix: 100,
            completed_at_unix: 200,
            apparatus: apparatus.to_string(),
            order_id: order_id.to_string(),
            action: queue_state::ApparatusQueueAction::Pause,
            status: OrderProgressBatchStatus::Paused,
            produced_qty: 10.0,
            uom: "kg".to_string(),
            qr_payload: "qr:batch-invalid-map-1".to_string(),
            label_item_code: order_id.to_string(),
            label_item_name: "Invalid map output".to_string(),
            executor_name: "Worker".to_string(),
            worker_role: "aparatchi".to_string(),
            worker_ref: "worker-1".to_string(),
            worker_display_name: "Worker".to_string(),
            wip_status: OrderProgressBatchWipStatus::Waiting,
            status_detail: OrderProgressBatchStatusDetail::default(),
            current_apparatus: apparatus.to_string(),
            current_location: "cell-1".to_string(),
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
            bobina_kg: None,
            finished_goods_meter: None,
            diameter: None,
            description: String::new(),
            payload_json: serde_json::json!({}),
        })
        .await;
    assert_eq!(write_result, Err(ProductionMapError::StoreFailed));

    // Transaction rollback: the batch write was NOT committed ...
    let batch_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_progress_batches WHERE batch_id = $1")
            .bind(batch_id)
            .fetch_one(&pool)
            .await
            .expect("batch must be absent after rollback");
    assert_eq!(batch_count, 0);
    assert!(
        store
            .progress_batch(batch_id)
            .await
            .expect("batch read")
            .is_none()
    );

    // ... and the lifecycle projection was NOT partially changed.
    let after: (String, String, String, String, i64) = sqlx::query_as(
        "SELECT lifecycle_status, operational_status, flow_status, stock_status,
                lifecycle_version
         FROM mini_production_maps WHERE id = $1",
    )
    .bind(order_id)
    .fetch_one(&pool)
    .await
    .expect("after projection");
    assert_eq!(
        (after.0, after.1, after.2, after.3, after.4),
        (before.0, before.1, before.2, before.3, before.4)
    );

    // Cleanup test database
    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
}

#[tokio::test]
async fn postgres_rejects_malformed_source_input_links_at_write_boundary() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://superuser@127.0.0.1:5432/postgres".to_string());
    let db_name = format!(
        "mini_rs_erp_test_malformed_source_links_{}",
        std::process::id()
    );
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

    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migrations");
    seed_standard_canonical_apparatus(&pool).await;

    let order_id = "order-malformed-links";
    let map = test_map(order_id, "9010", "MALFORMED");
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(order_id)
    .bind(&map.product_code)
    .bind(&map.title)
    .bind(serde_json::to_value(&map).expect("valid test map json"))
    .execute(&pool)
    .await
    .expect("seed order map");

    let apparatus = "apparatus:default:asset-010";
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let malformed_payloads = [
        // Duplicate sequence numbers.
        serde_json::json!({"source_input_links": [
            {"input_batch_id": "wip-a", "input_qr_payload": "qr:wip-a",
             "source_apparatus": apparatus, "source_kind": "progress_batch",
             "sequence_no": 1},
            {"input_batch_id": "wip-b", "input_qr_payload": "qr:wip-b",
             "source_apparatus": apparatus, "source_kind": "progress_batch",
             "sequence_no": 1},
        ]}),
        // Invalid source kind.
        serde_json::json!({"source_input_links": [
            {"input_batch_id": "wip-a", "input_qr_payload": "qr:wip-a",
             "source_apparatus": apparatus, "source_kind": "mystery",
             "sequence_no": 1},
        ]}),
        // Duplicate input batch IDs.
        serde_json::json!({"source_input_links": [
            {"input_batch_id": "wip-a", "input_qr_payload": "qr:wip-a",
             "source_apparatus": apparatus, "source_kind": "progress_batch",
             "sequence_no": 1},
            {"input_batch_id": "wip-a", "input_qr_payload": "qr:wip-a",
             "source_apparatus": apparatus, "source_kind": "progress_batch",
             "sequence_no": 2},
        ]}),
    ];
    for (index, payload) in malformed_payloads.into_iter().enumerate() {
        let batch_id = format!("batch-malformed-links-{index}");
        let write_result = store
            .put_order_progress_batch(OrderProgressBatch {
                batch_id: batch_id.clone(),
                revision: 1,
                session_id: "session-malformed-links".to_string(),
                started_at_unix: 100,
                completed_at_unix: 200,
                apparatus: apparatus.to_string(),
                order_id: order_id.to_string(),
                action: queue_state::ApparatusQueueAction::Pause,
                status: OrderProgressBatchStatus::Paused,
                produced_qty: 10.0,
                uom: "kg".to_string(),
                qr_payload: format!("qr:{batch_id}"),
                label_item_code: order_id.to_string(),
                label_item_name: "Malformed links output".to_string(),
                executor_name: "Worker".to_string(),
                worker_role: "aparatchi".to_string(),
                worker_ref: "worker-1".to_string(),
                worker_display_name: "Worker".to_string(),
                wip_status: OrderProgressBatchWipStatus::Waiting,
                status_detail: OrderProgressBatchStatusDetail::default(),
                current_apparatus: apparatus.to_string(),
                current_location: "cell-1".to_string(),
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
                bobina_kg: None,
                finished_goods_meter: None,
                diameter: None,
                description: String::new(),
                payload_json: payload,
            })
            .await;
        assert_eq!(write_result, Err(ProductionMapError::StoreFailed));
        let batch_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM mini_progress_batches WHERE batch_id = $1")
                .bind(&batch_id)
                .fetch_one(&pool)
                .await
                .expect("rejected batch must be absent");
        assert_eq!(batch_count, 0);
    }

    // Cleanup test database
    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
}
