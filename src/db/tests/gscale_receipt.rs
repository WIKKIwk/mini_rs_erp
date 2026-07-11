use crate::core::gscale::models::CreateMaterialReceiptDraftInput;
use crate::core::gscale::ports::MaterialReceiptStorePort;
use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_gscale_receipt::PostgresGscaleReceiptStore;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_gscale_precision"]
async fn postgres_gscale_receipt_preserves_nine_digit_decimal_quantity() {
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
    apply_foundation_migration(&pool)
        .await
        .expect("apply migrations");
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
