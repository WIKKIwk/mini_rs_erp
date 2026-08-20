use std::str::FromStr;

use sqlx::postgres::PgConnectOptions;

use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapEdge, ProductionMapNode, ProductionMapNodeKind,
};
use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_training_workspace::{
    PostgresTrainingWorkspaceStore, TrainingWorkspaceError,
};

const TRAINING_PRIMARY_ID: &str = "apparatus:test:training-primary";
const TRAINING_ALTERNATIVE_ID: &str = "apparatus:test:training-alternative";
const MISSING_PRIMARY_ID: &str = "apparatus:test:missing-primary";
const MISSING_ALTERNATIVE_ID: &str = "apparatus:test:missing-alternative";

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_training_workspace"]
async fn deleting_training_order_removes_only_its_queue_states() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_training_workspace";
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

    let test_options = PgConnectOptions::from_str(&admin_url)
        .expect("parse admin db url")
        .database(db_name);
    let pool = sqlx::PgPool::connect_with(test_options)
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    sqlx::query(
        "INSERT INTO mini_training_production_maps (id, order_number, map_json)
         VALUES ($1, $2, $3)",
    )
    .bind("training-1001")
    .bind("1001")
    .bind(serde_json::json!({"id": "training-1001"}))
    .execute(&pool)
    .await
    .expect("insert training map");
    sqlx::query(
        "INSERT INTO mini_training_queue_states
            (apparatus, canonical_apparatus_id, order_id, state)
         VALUES ('Flexo', 'apparatus:default:asset-005', 'training-1001', 'paused'),
                ('Laminatsiya 1', 'apparatus:default:asset-007', 'training-1001', 'pending'),
                ('Flexo', 'apparatus:default:asset-005', 'training-keep', 'pending')",
    )
    .execute(&pool)
    .await
    .expect("insert queue states");
    sqlx::query(
        "INSERT INTO mini_training_queue_events
            (event_id, apparatus, canonical_apparatus_id, order_id, action, from_state, to_state,
             actor_ref, actor_display_name)
         VALUES
            ('training-event-delete', 'Flexo', 'apparatus:default:asset-005', 'training-1001', 'complete', 'pending', 'completed', 'worker-1', 'Worker 1'),
            ('training-event-keep', 'Flexo', 'apparatus:default:asset-005', 'training-keep', 'start', 'pending', 'in_progress', 'worker-2', 'Worker 2')",
    )
    .execute(&pool)
    .await
    .expect("insert queue events");

    PostgresTrainingWorkspaceStore::new(pool.clone())
        .delete_order("training-1001")
        .await
        .expect("delete training order");

    let deleted_order_states: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_training_queue_states WHERE order_id = $1")
            .bind("training-1001")
            .fetch_one(&pool)
            .await
            .expect("count deleted order states");
    let unrelated_states: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_training_queue_states WHERE order_id = $1")
            .bind("training-keep")
            .fetch_one(&pool)
            .await
            .expect("count unrelated states");
    let deleted_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_training_queue_events WHERE order_id = $1")
            .bind("training-1001")
            .fetch_one(&pool)
            .await
            .expect("count deleted queue events");
    let unrelated_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_training_queue_events WHERE order_id = $1")
            .bind("training-keep")
            .fetch_one(&pool)
            .await
            .expect("count unrelated queue events");
    let deleted_map: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mini_training_production_maps WHERE id = $1")
            .bind("training-1001")
            .fetch_one(&pool)
            .await
            .expect("count deleted map");
    assert_eq!(deleted_order_states, 0);
    assert_eq!(unrelated_states, 1);
    assert_eq!(deleted_events, 0);
    assert_eq!(unrelated_events, 1);
    assert_eq!(deleted_map, 0);

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
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_training_apparatus_refs"]
async fn training_map_save_requires_existing_primary_and_alternative_apparatus_ids() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_training_apparatus_refs";
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

    let test_options = PgConnectOptions::from_str(&admin_url)
        .expect("parse admin db url")
        .database(db_name);
    let pool = sqlx::PgPool::connect_with(test_options)
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    for (id, name) in [
        (TRAINING_PRIMARY_ID, "Training primary"),
        (TRAINING_ALTERNATIVE_ID, "Training alternative"),
    ] {
        sqlx::query(
            "INSERT INTO mini_apparatus (id, name, base_name, kind, payload_json)
             VALUES ($1, $2, $2, 'test', '{}'::jsonb)",
        )
        .bind(id)
        .bind(name)
        .execute(&pool)
        .await
        .expect("insert canonical training apparatus");
    }

    let store = PostgresTrainingWorkspaceStore::new(pool.clone());
    let missing_primary = store
        .save_map(training_map(
            "training-missing-primary",
            MISSING_PRIMARY_ID,
            None,
            None,
            None,
        ))
        .await
        .expect_err("missing primary apparatus must reject the map");
    assert!(matches!(
        missing_primary,
        TrainingWorkspaceError::InvalidMap(detail) if detail.contains(MISSING_PRIMARY_ID)
    ));

    let missing_alternative = store
        .save_map(training_map(
            "training-missing-alternative",
            TRAINING_PRIMARY_ID,
            Some(MISSING_ALTERNATIVE_ID),
            Some(TRAINING_PRIMARY_ID),
            Some(MISSING_ALTERNATIVE_ID),
        ))
        .await
        .expect_err("missing alternative apparatus must reject the map");
    assert!(matches!(
        missing_alternative,
        TrainingWorkspaceError::InvalidMap(detail) if detail.contains(MISSING_ALTERNATIVE_ID)
    ));

    let missing_assigned_alternative = store
        .save_map(training_map(
            "training-missing-assigned-alternative",
            TRAINING_PRIMARY_ID,
            Some(TRAINING_ALTERNATIVE_ID),
            Some(TRAINING_PRIMARY_ID),
            None,
        ))
        .await
        .expect_err("missing alternative assignment must reject the map");
    assert!(matches!(
        missing_assigned_alternative,
        TrainingWorkspaceError::InvalidMap(_)
    ));

    let wrapped_primary = format!("  {TRAINING_PRIMARY_ID}  ");
    let wrapped_alternative = format!("  {TRAINING_ALTERNATIVE_ID}  ");
    let saved = store
        .save_map(training_map(
            "training-valid-apparatus-refs",
            &wrapped_primary,
            Some(&wrapped_alternative),
            Some(&wrapped_primary),
            Some(&wrapped_alternative),
        ))
        .await
        .expect("existing canonical apparatus IDs must save");
    assert_eq!(saved.map.id, "training-valid-apparatus-refs");

    let persisted_apparatus_ids: Vec<(String, String)> = sqlx::query_as(
        "SELECT nodes.node->>'apparatus_id',
                nodes.node->>'alternative_assigned_apparatus_id'
         FROM mini_training_production_maps map_row
         CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') AS nodes(node)
         WHERE map_row.id = 'training-valid-apparatus-refs'
           AND nodes.node->>'kind' = 'apparatus'
         ORDER BY nodes.node->>'id'",
    )
    .fetch_all(&pool)
    .await
    .expect("read persisted training apparatus IDs");
    assert_eq!(persisted_apparatus_ids.len(), 2);
    assert!(persisted_apparatus_ids.iter().any(|(apparatus, assigned)| {
        apparatus == TRAINING_PRIMARY_ID && assigned == TRAINING_PRIMARY_ID
    }));
    assert!(persisted_apparatus_ids.iter().any(|(apparatus, assigned)| {
        apparatus == TRAINING_ALTERNATIVE_ID && assigned == TRAINING_ALTERNATIVE_ID
    }));

    let persisted_valid_map: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM mini_training_production_maps
         WHERE id = 'training-valid-apparatus-refs'",
    )
    .fetch_one(&pool)
    .await
    .expect("count valid training map");
    let persisted_rejected_maps: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM mini_training_production_maps
         WHERE id IN (
             'training-missing-primary',
             'training-missing-alternative',
             'training-missing-assigned-alternative'
         )",
    )
    .fetch_one(&pool)
    .await
    .expect("count rejected training maps");
    assert_eq!(persisted_valid_map, 1);
    assert_eq!(persisted_rejected_maps, 0);

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

fn training_map(
    id: &str,
    primary_id: &str,
    alternative_id: Option<&str>,
    assigned_primary_id: Option<&str>,
    assigned_alternative_id: Option<&str>,
) -> ProductionMapDefinition {
    let mut primary = training_node(
        "primary",
        ProductionMapNodeKind::Apparatus,
        "Training primary",
        primary_id,
    );
    let mut nodes = vec![
        training_node("start", ProductionMapNodeKind::Start, "Start", ""),
        primary,
    ];
    let mut edges = vec![ProductionMapEdge {
        from: "start".to_string(),
        to: "primary".to_string(),
        branch: String::new(),
    }];

    if let Some(alternative_id) = alternative_id {
        let assigned_primary_id = assigned_primary_id.unwrap_or_default();
        let assigned_alternative_id = assigned_alternative_id.unwrap_or_default();
        primary = nodes.pop().expect("primary node");
        primary.alternative_group_id = "training-alternatives".to_string();
        primary.alternative_group_label = "Training alternatives".to_string();
        primary.alternative_assigned_title = "Training alternative".to_string();
        primary.alternative_assigned_apparatus_id = assigned_primary_id.to_string();
        nodes.push(primary);

        let mut alternative = training_node(
            "alternative",
            ProductionMapNodeKind::Apparatus,
            "Training alternative",
            alternative_id,
        );
        alternative.alternative_group_id = "training-alternatives".to_string();
        alternative.alternative_group_label = "Training alternatives".to_string();
        alternative.alternative_assigned_title = "Training alternative".to_string();
        alternative.alternative_assigned_apparatus_id = assigned_alternative_id.to_string();
        nodes.push(alternative);
        edges.push(ProductionMapEdge {
            from: "primary".to_string(),
            to: "alternative".to_string(),
            branch: String::new(),
        });
    }

    let final_node_id = if alternative_id.is_some() {
        "alternative"
    } else {
        "primary"
    };
    nodes.push(training_node("end", ProductionMapNodeKind::End, "End", ""));
    edges.push(ProductionMapEdge {
        from: final_node_id.to_string(),
        to: "end".to_string(),
        branch: String::new(),
    });

    ProductionMapDefinition {
        id: id.to_string(),
        product_code: format!("{id}-product"),
        title: "Training map apparatus reference test".to_string(),
        code: format!("{id}-code"),
        order_number: format!("ORDER-{id}"),
        customer_name: String::new(),
        roll_count: None,
        width_mm: None,
        order_kg: None,
        base_length: None,
        nodes,
        edges,
    }
}

fn training_node(
    id: &str,
    kind: ProductionMapNodeKind,
    title: &str,
    apparatus_id: &str,
) -> ProductionMapNode {
    ProductionMapNode {
        id: id.to_string(),
        kind,
        title: title.to_string(),
        apparatus_id: apparatus_id.to_string(),
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
        y: 0.0,
    }
}
