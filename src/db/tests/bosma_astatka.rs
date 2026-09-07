use crate::core::production_map::{BosmaAstatkaReport, ProductionMapStorePort};
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_production_map::PostgresProductionMapStore;

#[tokio::test]
async fn bosma_astatka_postgres_roundtrip_without_production_output() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
    let db_name = format!(
        "mini_rs_erp_test_bosma_astatka_{:016x}",
        rand::random::<u64>()
    );
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    super::seed_standard_canonical_apparatus(&pool).await;
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, code, map_json)
        VALUES ('zakaz-bosma-astatka-db', 'BAST', 'Bosma astatka', 'BAST', $1)",
    )
    .bind(serde_json::json!({"id":"zakaz-bosma-astatka-db"}))
    .execute(&pool)
    .await
    .unwrap();
    let report: BosmaAstatkaReport = serde_json::from_value(serde_json::json!({
        "report_id": "bosma-astatka:db-roundtrip", "order_id": "zakaz-bosma-astatka-db",
        "apparatus": "apparatus:default:bosma_8", "from_at_unix": 100, "to_at_unix": 200,
        "total_waste": 0, "finished_goods_meter": 80, "finished_goods_kg": 12, "bobina_kg": 1,
        "description": "Must remain audit only",
        "returned_paint": {
            "id": "bosma-astatka:db-roundtrip", "order_id": "zakaz-bosma-astatka-db",
            "order_code": "BAST", "order_name": "Bosma astatka", "apparatus": "apparatus:default:bosma_8",
            "sender_role": "aparatchi", "sender_ref": "bosmachi-1", "sender_display_name": "Bosmachi",
            "items": [{"usage":"astatka", "category":"colors", "name":"Oq", "values":{"Mix":"1.25"}}],
            "status": "completed", "created_at_unix": 200
        }
    })).unwrap();
    let store = PostgresProductionMapStore::new(pool.clone());
    store
        .put_bosma_astatka_report(report.clone())
        .await
        .unwrap();
    let reloaded = PostgresProductionMapStore::new(pool.clone())
        .bosma_astatka_reports_for_order(&report.order_id)
        .await
        .unwrap();
    assert_eq!(reloaded, vec![report.clone()]);
    assert!(
        store
            .progress_batches_for_order(&report.order_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .order_run_sessions_for_order(&report.order_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(store.apparatus_queue_states().await.unwrap().is_empty());
    assert!(
        store.put_bosma_astatka_report(report).await.is_err(),
        "duplicate report IDs must not overwrite audit data"
    );
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db_name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
