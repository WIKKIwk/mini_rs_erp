use std::sync::Arc;

use super::{
    RawMaterialStockTransitionRow, apply_raw_material_stock_transitions_tx,
    ensure_stock_transition_rows_affected, stock_transition_event_draft,
};
use crate::core::apparatus_standard::{
    ApparatusId, ProcessTechnology,
    service::CanonicalApparatusService,
    test_support::{TestApparatusSpec, canonical_draft},
};
use crate::core::production_map::{
    ProductionMapError, QueueActionActor, RawMaterialStockTransition,
    RawMaterialStockTransitionKind,
};
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;

#[test]
fn consumed_transition_rejects_a_silent_zero_row_update() {
    assert_eq!(
        ensure_stock_transition_rows_affected(RawMaterialStockTransitionKind::Consumed, 1, 0,),
        Err(ProductionMapError::RawMaterialStockUnavailable)
    );
    assert_eq!(
        ensure_stock_transition_rows_affected(RawMaterialStockTransitionKind::Consumed, 2, 2,),
        Ok(())
    );
    assert_eq!(
        ensure_stock_transition_rows_affected(RawMaterialStockTransitionKind::InUse, 2, 1,),
        Err(ProductionMapError::RawMaterialStockUnavailable)
    );
}

#[test]
fn consumption_event_identity_is_scoped_to_canonical_apparatus() {
    let row = RawMaterialStockTransitionRow {
        id: "raw:001".to_string(),
        warehouse: "Raw warehouse".to_string(),
        item_code: "FILM-001".to_string(),
        item_name: "Film".to_string(),
        barcode: "RM-001".to_string(),
        qty: 4.5,
        uom: "kg".to_string(),
        status: "consumed".to_string(),
        reserved_order_id: "ORDER-001".to_string(),
        source_receipt_id: "receipt-001".to_string(),
    };
    let actor = QueueActionActor {
        role: "admin".to_string(),
        ref_: "admin-001".to_string(),
        display_name: "Admin".to_string(),
    };
    let first = stock_transition_event_draft(
        RawMaterialStockTransitionKind::Consumed,
        &row,
        Some("in_use".to_string()),
        "ORDER-001",
        &actor,
        "apparatus:catalog:first",
        None,
    );
    let second = stock_transition_event_draft(
        RawMaterialStockTransitionKind::Consumed,
        &row,
        Some("in_use".to_string()),
        "ORDER-001",
        &actor,
        "apparatus:catalog:second",
        None,
    );

    assert_ne!(first.idempotency_key, second.idempotency_key);
    assert_eq!(first.qty_delta, -4.5);
    assert_eq!(
        first.payload_json["apparatus_id"],
        serde_json::json!("apparatus:catalog:first")
    );
}

#[tokio::test]
async fn completion_consumes_in_use_and_unlinks_available_material_atomically() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_complete_material_settlement";
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
        .expect("apply migrations");
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
             VALUES ('ORDER-COMPLETE', 'COMPLETE', 'Completion material test', '{}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("seed production map");
    CanonicalApparatusService::new(Arc::new(PostgresCanonicalApparatusRepository::new(
        pool.clone(),
    )))
    .seed_for_test(
        ApparatusId::new("apparatus:default:bosma_7".to_string())
            .expect("canonical apparatus id"),
        canonical_draft(&TestApparatusSpec::print(
            "apparatus:default:bosma_7",
            "Bosma 7",
            ProcessTechnology::Rotogravure,
            Some(7),
        )),
    )
    .await
    .expect("seed canonical apparatus");
    sqlx::query(
        "INSERT INTO mini_raw_material_stock (
                 id, warehouse, item_code, item_name, barcode, qty, uom,
                 status, reserved_order_id, source_receipt_id, payload_json
             ) VALUES
                 ('raw:complete-used', 'Warehouse A', 'BOPP', 'Used roll',
                  'COMPLETE-USED', 11, 'kg', 'in_use', 'ORDER-COMPLETE',
                  'receipt-used', '{\"physical_marker\":\"used\"}'::jsonb),
                 ('raw:complete-unused', 'Warehouse B', 'BOPP', 'Unused roll',
                  'COMPLETE-UNUSED', 23, 'kg', 'available', 'ORDER-COMPLETE',
                  'receipt-unused',
                  '{\"physical_marker\":\"keep\",\"in_use_order_id\":\"ORDER-COMPLETE\"}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("seed raw material stock");
    sqlx::query(
        "INSERT INTO mini_factory_locations (id, name)
             VALUES ('state:complete-unused', 'Completion unused state')",
    )
    .execute(&pool)
    .await
    .expect("seed unused stock state");
    sqlx::query(
        "INSERT INTO mini_inventory_placements (
                 asset_kind, asset_ref, physical_location_id
             ) VALUES (
                 'raw_material', 'raw:complete-unused',
                 'inventory_location:state:state:complete-unused'
             )",
    )
    .execute(&pool)
    .await
    .expect("place unused stock in state");
    for barcode in ["COMPLETE-USED", "COMPLETE-UNUSED"] {
        sqlx::query(
            "INSERT INTO mini_raw_material_assignments (
                     barcode, order_id, apparatus, canonical_apparatus_id,
                     item_code, item_group, payload_json
                 ) VALUES ($1, 'ORDER-COMPLETE', 'apparatus:default:bosma_7',
                           'apparatus:default:bosma_7', 'BOPP', 'rulon',
                           jsonb_build_object(
                               'barcode', $1::text,
                               'order_id', 'ORDER-COMPLETE',
                               'apparatus', 'apparatus:default:bosma_7',
                               'apparatus_id', 'apparatus:default:bosma_7',
                               'item_code', 'BOPP',
                               'item_group', 'rulon'
                           ))",
        )
        .bind(barcode)
        .execute(&pool)
        .await
        .expect("seed raw material assignment");
    }

    let mut tx = pool.begin().await.expect("begin settlement");
    let outcome = apply_raw_material_stock_transitions_tx(
        &mut tx,
        &[RawMaterialStockTransition::new(
            RawMaterialStockTransitionKind::Complete,
            vec!["COMPLETE-USED".to_string(), "COMPLETE-UNUSED".to_string()],
            "ORDER-COMPLETE",
        )],
        &QueueActionActor {
            role: "aparatchi".to_string(),
            ref_: "WORKER-1".to_string(),
            display_name: "Worker".to_string(),
        },
        "apparatus:default:bosma_7",
    )
    .await
    .expect("settle completion materials");
    tx.commit().await.expect("commit settlement");

    assert_eq!(outcome.unused_unlinks.len(), 1);
    assert_eq!(outcome.unused_unlinks[0].barcode, "COMPLETE-UNUSED");
    let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT barcode, status, reserved_order_id, warehouse,
                    COALESCE(payload_json->>'physical_marker', ''),
                    COALESCE(payload_json->>'in_use_order_id', '')
             FROM mini_raw_material_stock
             WHERE barcode IN ('COMPLETE-USED', 'COMPLETE-UNUSED')
             ORDER BY barcode",
    )
    .fetch_all(&pool)
    .await
    .expect("settled stock rows");
    assert_eq!(
        rows,
        vec![
            (
                "COMPLETE-UNUSED".to_string(),
                "available".to_string(),
                "".to_string(),
                "Warehouse B".to_string(),
                "keep".to_string(),
                "".to_string(),
            ),
            (
                "COMPLETE-USED".to_string(),
                "consumed".to_string(),
                "ORDER-COMPLETE".to_string(),
                "Warehouse A".to_string(),
                "used".to_string(),
                "".to_string(),
            ),
        ]
    );
    let assignments: Vec<String> = sqlx::query_scalar(
        "SELECT barcode FROM mini_raw_material_assignments ORDER BY barcode",
    )
    .fetch_all(&pool)
    .await
    .expect("remaining assignments");
    assert_eq!(assignments, vec!["COMPLETE-USED".to_string()]);
    let consumed_events: Vec<String> = sqlx::query_scalar(
        "SELECT barcode FROM mini_raw_material_events
             WHERE event_type = 'consumption_posted'
             ORDER BY barcode",
    )
    .fetch_all(&pool)
    .await
    .expect("consumption audit");
    assert_eq!(consumed_events, vec!["COMPLETE-USED".to_string()]);
    let unused_location: String = sqlx::query_scalar(
        "SELECT physical_location_id
             FROM mini_inventory_placements
             WHERE asset_kind = 'raw_material'
               AND asset_ref = 'raw:complete-unused'",
    )
    .fetch_one(&pool)
    .await
    .expect("unused stock remains in its physical state");
    assert_eq!(
        unused_location,
        "inventory_location:state:state:complete-unused"
    );

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
