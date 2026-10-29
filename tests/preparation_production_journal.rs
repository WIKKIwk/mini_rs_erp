use std::sync::Arc;

use mini_rs_erp::core::rps_batch::models::{RpsBatchPrintEntry, RpsBatchSession};
use mini_rs_erp::core::rps_batch::ports::RpsBatchStorePort;
use mini_rs_erp::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    canonical_apparatus_service,
};
use mini_rs_erp::db::postgres_preparation::PostgresPreparationStore;
use mini_rs_erp::db::postgres_rps_batch::PostgresRpsBatchStore;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgConnectOptions};

#[tokio::test]
#[ignore = "requires maintenance PostgreSQL access; creates an isolated database"]
async fn journal_groups_confirmed_rolls_by_order_preserves_recipe_and_scope_and_pages() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".into());
    let admin = PgPool::connect(&url).await.unwrap();
    let database = format!(
        "mini_test_production_journal_{:016x}",
        rand::random::<u64>()
    );
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&database))
        .await
        .unwrap();
    let checks = pool.clone();
    let result = tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move { run_checks(checks).await }).await
        })
        .await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn run_checks(pool: PgPool) {
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    canonical_apparatus_service(pool.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT to_regclass('public.idx_mini_gscale_receipts_preparation_owner') IS NOT NULL"
        )
        .fetch_one(&pool)
        .await
        .unwrap()
    );
    sqlx::raw_sql("INSERT INTO mini_orders(id,code,order_number,customer_name,product_name) VALUES
        ('order-a','0317','0317','Silver choy','Tea packaging'),('order-b','0318','0318','Customer B','Other packaging');")
        .execute(&pool).await.unwrap();
    let recipe = json!({"schema_version":1,"variant_id":"variant-lg","variant_name":"LG","material_name":"PE PR",
        "formulas":[{"name":"A","share_percent":"100","lines":[{"item_code":"S1","name":"Seriya 1","percent":"88"},{"item_code":"S2","name":"Seriya 2","percent":"12"}]}]});
    let store = Arc::new(PostgresRpsBatchStore::new(pool.clone()));
    let mut batch = RpsBatchSession {
        id: "batch-a".into(),
        owner_key: "tayyorlov_masteri:master-1".into(),
        owner_role: "tayyorlov_masteri".into(),
        owner_ref: "master-1".into(),
        item_code: "PE PR".into(),
        item_name: "PE PR".into(),
        warehouse: "Own W".into(),
        active: false,
        order_assignment: Some(json!({"order_id":"order-a","preparation_formula":recipe})),
        created_at: "2026-10-28T05:00:00Z".into(),
        updated_at: "2026-10-28T05:10:00Z".into(),
        prints: vec![print("EPC1"), print("EPC1"), print("EPC2")],
        ..Default::default()
    };
    store.complete(&batch).await.unwrap(); // persists both current and history
    let duplicated = PostgresPreparationStore::new(pool.clone())
        .production_journal_orders("master-1", "", "all", 0, 50)
        .await
        .unwrap();
    assert_eq!(
        duplicated["orders"][0]["batch_count"], 1,
        "the same completed session in current and history counts once"
    );
    batch.id = "batch-live".into();
    batch.active = true;
    batch.prints.clear();
    batch.updated_at = "2026-10-29T05:00:00Z".into();
    store.put(&batch).await.unwrap();
    for (epc, order, qty, status, owner) in [
        ("EPC1", "order-a", 99, "submitted", "master-1"),
        ("EPC2", "order-a", 20, "submitted", "master-1"),
        ("EPC3", "order-a", 5, "submitted", "master-1"),
        ("DRAFT", "order-a", 200, "draft", "master-1"),
        ("OTHER", "order-a", 300, "submitted", "master-2"),
        ("EPC-B", "order-b", 7, "submitted", "master-1"),
    ] {
        let payload = json!({"actor_role":"tayyorlov_masteri","actor_ref":owner,"item_name":"PE PR",
            "order_assignment":{"order_id":order,"preparation_formula":if epc=="EPC1" {recipe.clone()} else {Value::Null}}});
        sqlx::query("INSERT INTO mini_gscale_receipts(name,status,item_code,warehouse,qty,barcode,payload_json,length_m,width_mm,micron,created_at,submitted_at)
            VALUES($1,$2,'PE PR','Own W',$3,$4,$5,125,495,50,'2026-10-28T05:05:00Z','2026-10-28T05:05:00Z')")
            .bind(format!("receipt-{epc}")).bind(status).bind(qty as f64).bind(epc).bind(payload).execute(&pool).await.unwrap();
    }
    // Original production event remains authoritative after a receipt correction.
    sqlx::query("INSERT INTO mini_raw_material_events(event_id,idempotency_key,event_type,warehouse,barcode,item_code,item_name,
        qty_delta,order_id,actor_role,actor_ref,source_type,source_id,payload_json,occurred_at)
        VALUES('journal-event','journal-event','receipt_posted','Original W','EPC1','PE PR','Original PE PR',10,
            'order-a','tayyorlov_masteri','master-1','gscale_receipt','receipt-EPC1',$1,'2026-10-28T05:05:00Z')")
        .bind(json!({"length_m":100,"width_mm":490,"micron":40,"preparation_formula":recipe})).execute(&pool).await.unwrap();
    let journal = PostgresPreparationStore::new(pool.clone());
    let list = journal
        .production_journal_orders("master-1", "", "all", 0, 1)
        .await
        .unwrap();
    assert_eq!(list["orders"][0]["order_id"], "order-a");
    assert_eq!(list["orders"][0]["roll_count"], 3);
    assert_eq!(list["orders"][0]["batch_count"], 3); // completed, active-empty, legacy receipt
    assert_eq!(list["orders"][0]["active_batch_count"], 1);
    assert_eq!(number(&list["orders"][0]["total_net_kg"]), 35.0);
    assert_eq!(number(&list["orders"][0]["total_length_m"]), 350.0);
    assert_eq!(list["has_more"], true);
    assert_eq!(list["next_offset"], 1);
    let next = journal
        .production_journal_orders("master-1", "", "all", 1, 1)
        .await
        .unwrap();
    assert_eq!(next["orders"][0]["order_id"], "order-b");
    assert_eq!(next["has_more"], false);
    for search in ["0317", "Tea", "Silver", "PE PR", "LG"] {
        assert_eq!(
            journal
                .production_journal_orders("master-1", search, "all", 0, 50)
                .await
                .unwrap()["orders"][0]["order_id"],
            "order-a"
        );
    }
    assert_eq!(
        journal
            .production_journal_orders("master-1", "", "active", 0, 50)
            .await
            .unwrap()["orders"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        journal
            .production_journal_orders("master-1", "", "completed", 0, 50)
            .await
            .unwrap()["orders"][0]["order_id"],
        "order-b"
    );
    let detail = journal
        .production_journal_order("master-1", "order-a", 0, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail["batches"][0]["batch_id"], "batch:batch-live");
    assert_eq!(detail["batches"][0]["roll_count"], 0);
    assert_eq!(detail["has_more"], true);
    let rolls = journal
        .production_journal_batch("master-1", "batch:batch-a", 0, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rolls["batch"]["roll_count"], 2);
    assert_eq!(rolls["has_more"], true);
    let all = journal
        .production_journal_batch("master-1", "batch:batch-a", 0, 50)
        .await
        .unwrap()
        .unwrap();
    let original = all["rolls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|roll| roll["epc"] == "EPC1")
        .unwrap();
    assert_eq!(number(&original["net_kg"]), 10.0);
    assert_eq!(number(&original["width_mm"]), 490.0);
    assert_eq!(original["preparation_formula"], recipe);
    assert!(
        journal
            .production_journal_batch("master-2", "batch:batch-a", 0, 50)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        journal
            .production_journal_order("master-3", "order-a", 0, 50)
            .await
            .unwrap()
            .is_none()
    );
    let legacy = journal
        .production_journal_batch("master-1", "receipt:EPC3", 0, 50)
        .await
        .unwrap()
        .unwrap();
    assert!(legacy["batch"]["preparation_formula"].is_null());
    assert_eq!(legacy["rolls"][0]["epc"], "EPC3");
    // No current warehouse or formula assignments exist in this fixture: historical ownership suffices.
    assert_eq!(
        journal
            .production_journal_orders("master-2", "", "all", 0, 50)
            .await
            .unwrap()["orders"][0]["roll_count"],
        1
    );
}

fn print(epc: &str) -> RpsBatchPrintEntry {
    RpsBatchPrintEntry {
        epc: epc.into(),
        gross_qty: if epc == "EPC2" { 22.0 } else { 12.0 },
        net_qty: if epc == "EPC2" { 20.0 } else { 10.0 },
        printed_at: "2026-10-28T05:05:00Z".into(),
        ..Default::default()
    }
}
fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}
