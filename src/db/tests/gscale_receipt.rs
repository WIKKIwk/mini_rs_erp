use crate::core::gscale::models::{CreateMaterialReceiptDraftInput, RawMaterialStockUpdateInput};
use crate::core::gscale::ports::MaterialReceiptStorePort;
use crate::db::postgres::{apply_foundation_migration, apply_postgres_migrations_through};
use crate::db::postgres_gscale_receipt::PostgresGscaleReceiptStore;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_gscale_precision"]
async fn postgres_gscale_receipt_preserves_precision_and_supports_stock_corrections() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_gscale_precision";
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
    apply_postgres_migrations_through(&pool, 13)
        .await
        .expect("apply pre-correction migrations");
    let store = PostgresGscaleReceiptStore::new(pool.clone());
    let draft = store
        .create_material_receipt_draft(CreateMaterialReceiptDraftInput {
            item_code: "ITEM-PRECISION".to_string(),
            item_name: "Precision material".to_string(),
            warehouse: "Sklad U".to_string(),
            qty: 13.00003,
            barcode: "PRECISION-0001".to_string(),
            actor_role: "material_taminotchi".to_string(),
            actor_ref: "MAT-001".to_string(),
            actor_display_name: "Material".to_string(),
        })
        .await
        .expect("create receipt draft");
    store
        .submit_stock_entry_draft(&draft.name)
        .await
        .expect("submit receipt");

    let receipt_qty: String = sqlx::query_scalar(
        "SELECT qty::text FROM mini_gscale_receipts WHERE barcode = 'PRECISION-0001'",
    )
    .fetch_one(&pool)
    .await
    .expect("receipt qty");
    let stock_qty: String = sqlx::query_scalar(
        "SELECT qty::text FROM mini_raw_material_stock WHERE barcode = 'PRECISION-0001'",
    )
    .fetch_one(&pool)
    .await
    .expect("stock qty");
    let event_qty: String = sqlx::query_scalar(
        "SELECT qty_delta::text FROM mini_raw_material_events
         WHERE barcode = 'PRECISION-0001' AND event_type = 'receipt_posted'",
    )
    .fetch_one(&pool)
    .await
    .expect("event qty");
    assert_eq!(receipt_qty, "13.000030000");
    assert_eq!(stock_qty, receipt_qty);
    assert_eq!(event_qty, receipt_qty);

    apply_foundation_migration(&pool)
        .await
        .expect("upgrade existing stock ledger with correction migration");
    apply_foundation_migration(&pool)
        .await
        .expect("migrations remain idempotent");
    let migration_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mini_schema_migrations")
        .fetch_one(&pool)
        .await
        .expect("migration count");
    assert_eq!(migration_count, 14);

    let increased = store
        .update_raw_material_stock(RawMaterialStockUpdateInput {
            barcode: draft.barcode.clone(),
            item_code: "ITEM-CORRECTED".to_string(),
            item_name: "Corrected material".to_string(),
            qty: 14.0,
            actor_role: "material_taminotchi".to_string(),
            actor_ref: "MAT-001".to_string(),
            actor_display_name: "Material".to_string(),
        })
        .await
        .expect("increase corrected stock");
    assert_eq!(increased.barcode, draft.barcode);
    assert_eq!(increased.source_receipt_id, draft.name);
    assert_eq!(increased.qty, 14.0);

    store
        .update_raw_material_stock(RawMaterialStockUpdateInput {
            barcode: draft.barcode.clone(),
            item_code: "ITEM-CORRECTED".to_string(),
            item_name: "Corrected material".to_string(),
            qty: 12.5,
            actor_role: "material_taminotchi".to_string(),
            actor_ref: "MAT-001".to_string(),
            actor_display_name: "Material".to_string(),
        })
        .await
        .expect("decrease corrected stock");
    let renamed = store
        .update_raw_material_stock(RawMaterialStockUpdateInput {
            barcode: draft.barcode.clone(),
            item_code: "ITEM-RENAMED".to_string(),
            item_name: "Renamed material".to_string(),
            qty: 12.5,
            actor_role: "material_taminotchi".to_string(),
            actor_ref: "MAT-001".to_string(),
            actor_display_name: "Material".to_string(),
        })
        .await
        .expect("rename corrected stock without quantity change");
    assert_eq!(renamed.item_code, "ITEM-RENAMED");
    assert_eq!(renamed.item_name, "Renamed material");
    assert_eq!(renamed.qty, 12.5);
    assert_eq!(renamed.barcode, "PRECISION-0001");
    assert_eq!(renamed.source_receipt_id, draft.name);

    let (receipt_name, receipt_barcode, receipt_item_code, receipt_qty, receipt_item_name): (
        String,
        String,
        String,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT name, barcode, item_code, qty::text,
                payload_json->>'item_name'
         FROM mini_gscale_receipts
         WHERE barcode = 'PRECISION-0001'",
    )
    .fetch_one(&pool)
    .await
    .expect("corrected receipt identity");
    assert_eq!(receipt_name, draft.name);
    assert_eq!(receipt_barcode, draft.barcode);
    assert_eq!(receipt_item_code, "ITEM-RENAMED");
    assert_eq!(receipt_qty, "12.500000000");
    assert_eq!(receipt_item_name, "Renamed material");

    let corrections: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT event_type, source_type, qty_delta::text,
                stock_status_before, stock_status_after
         FROM mini_raw_material_events
         WHERE barcode = 'PRECISION-0001'
           AND event_type = 'stock_corrected'
         ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .expect("stock correction audit events");
    assert_eq!(corrections.len(), 3);
    assert_eq!(corrections[0].0, "stock_corrected");
    assert_eq!(corrections[0].1, "stock_correction");
    assert_eq!(corrections[0].2, "0.999970000");
    assert_eq!(corrections[1].2, "-1.500000000");
    assert_eq!(corrections[2].2, "0.000000000");
    assert!(
        corrections
            .iter()
            .all(|event| event.3 == "available" && event.4 == "available")
    );
    let invalid_correction = sqlx::query(
        "INSERT INTO mini_raw_material_events (
             event_id, idempotency_key, event_type, warehouse, barcode,
             item_code, item_name, qty_delta, uom,
             stock_status_before, stock_status_after, order_id, apparatus,
             actor_role, actor_ref, actor_display_name,
             owner_role, owner_ref, owner_display_name,
             source_type, source_id, source_line_ref, correlation_id, payload_json
         )
         SELECT event_id || '-invalid', idempotency_key || '-invalid', event_type,
                warehouse, barcode, item_code, item_name, qty_delta, uom,
                stock_status_before, stock_status_after, order_id, apparatus,
                actor_role, actor_ref, actor_display_name,
                owner_role, owner_ref, owner_display_name,
                'system', source_id, source_line_ref, correlation_id, payload_json
         FROM mini_raw_material_events
         WHERE barcode = 'PRECISION-0001'
           AND event_type = 'stock_corrected'
         ORDER BY id
         LIMIT 1",
    )
    .execute(&pool)
    .await
    .expect_err("stock correction source must remain paired with its event type");
    assert_eq!(
        invalid_correction
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("mini_rme_stock_correction_consistent")
    );

    let invalid = store
        .create_material_receipt_draft(CreateMaterialReceiptDraftInput {
            item_code: "ITEM-PRECISION".to_string(),
            warehouse: "Sklad U".to_string(),
            qty: f64::NAN,
            barcode: "PRECISION-NAN".to_string(),
            ..CreateMaterialReceiptDraftInput::default()
        })
        .await;
    assert!(invalid.is_err());

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
