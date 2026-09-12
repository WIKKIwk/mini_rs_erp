use crate::core::{
    auth::models::{Principal, PrincipalRole},
    preparation::*,
};
use crate::db::{
    postgres::apply_foundation_migration, postgres_preparation::PostgresPreparationStore,
};
use serde_json::{Value, json};
use sqlx::postgres::PgConnectOptions;

fn consume(order: &str, code: &str, percent: &str) -> ConsumptionCreate {
    ConsumptionCreate {
        request_id: format!("consume-{order}"),
        warehouse: "Preparation W".into(),
        order_id: order.into(),
        expected_order_kg: "1000".into(),
        lines: vec![ConsumptionLine {
            item_code: code.into(),
            percent: percent.into(),
        }],
    }
}

#[tokio::test]
async fn preparation_postgres_partial_fifo_atomic_retry_concurrency_and_scope() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".into());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let db = format!("mini_rs_erp_test_prep_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = url.parse::<PgConnectOptions>().unwrap().database(&db);
    let pool = sqlx::PgPool::connect_with(options).await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO mini_system_users(id,role,name,phone) VALUES
            ('prep-1','tayyorlov_masteri','Master','901234567'),('prep-2','tayyorlov_masteri','Other','901234568');
        INSERT INTO mini_warehouses(id,name) VALUES ('prep-w','Preparation W'),('other-w','Other W');
        INSERT INTO mini_warehouse_assignments(assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
            VALUES ('warehouse','Preparation W','Preparation W','tayyorlov_masteri','prep-1');")
        .execute(&pool).await.unwrap();
    for id in [
        "order1", "order2", "rollback", "race1", "race2", "stale", "closed", "reserved",
    ] {
        sqlx::query("INSERT INTO mini_production_maps(id,product_code,title,code,map_json) VALUES ($1,'P','Test order',$1,$2)")
            .bind(id).bind(json!({"id":id,"order_kg":1000})).execute(&pool).await.unwrap();
    }
    let actor = Principal {
        role: PrincipalRole::TayyorlovMasteri,
        display_name: "Master".into(),
        legal_name: String::new(),
        ref_: "prep-1".into(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let store = PostgresPreparationStore::new(pool.clone());
    let material_input = MaterialCreate {
        request_id: "material-001".into(),
        name: "Kley".into(),
    };
    let material = store
        .create_material(&actor, material_input.clone())
        .await
        .unwrap();
    assert_eq!(
        material,
        store.create_material(&actor, material_input).await.unwrap()
    );
    let code = material["item_code"].as_str().unwrap();
    assert!(matches!(
        store
            .create_material(
                &actor,
                MaterialCreate {
                    request_id: "material-002".into(),
                    name: " kley ".into()
                }
            )
            .await,
        Err(PreparationError::Conflict(_))
    ));
    let receipt = ReceiptCreate {
        request_id: "receipt-001".into(),
        item_code: code.into(),
        warehouse: "Preparation W".into(),
        kg: "60".into(),
    };
    let (r1, r2) = tokio::join!(
        store.receive(&actor, receipt.clone()),
        store.receive(&actor, receipt.clone())
    );
    assert_eq!(r1.unwrap(), r2.unwrap());
    assert!(matches!(
        store
            .receive(
                &actor,
                ReceiptCreate {
                    kg: "61".into(),
                    ..receipt.clone()
                }
            )
            .await,
        Err(PreparationError::Conflict(_))
    ));
    store
        .receive(
            &actor,
            ReceiptCreate {
                request_id: "receipt-002".into(),
                kg: "50".into(),
                ..receipt.clone()
            },
        )
        .await
        .unwrap();
    let result = store
        .consume(&actor, consume("order1", code, "3"))
        .await
        .unwrap();
    assert_eq!(result["lines"][0]["kg"], "30.000000");
    assert_eq!(
        result,
        store
            .consume(&actor, consume("order1", code, "3"))
            .await
            .unwrap()
    );
    assert!(matches!(
        store
            .consume(
                &actor,
                ConsumptionCreate {
                    request_id: "another-save-key".into(),
                    ..consume("order1", code, "3")
                }
            )
            .await,
        Err(PreparationError::Conflict(_))
    ));
    store
        .consume(&actor, consume("order2", code, "4"))
        .await
        .unwrap();
    let quantities: Vec<String> = sqlx::query_scalar(
        "SELECT qty::text FROM mini_raw_material_stock WHERE status='available'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(quantities, vec!["40.000000"]);
    let allocations: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_preparation_allocations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(allocations, 3);
    let before: Value = store.snapshot("prep-1").await.unwrap();
    assert!(
        before["orders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["id"] == "order1" && o["saved"] == true)
    );
    assert!(
        store.snapshot("prep-2").await.unwrap()["materials"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let other = Principal {
        ref_: "prep-2".into(),
        ..actor.clone()
    };
    assert!(matches!(
        store.receive(&other, receipt.clone()).await,
        Err(PreparationError::Forbidden)
    ));
    assert!(matches!(
        store
            .receive(
                &actor,
                ReceiptCreate {
                    request_id: "receipt-other-w".into(),
                    warehouse: "Other W".into(),
                    ..receipt.clone()
                }
            )
            .await,
        Err(PreparationError::Forbidden)
    ));

    // Roll back an earlier line's stock update and event when a later line fails.
    let second = store
        .create_material(
            &actor,
            MaterialCreate {
                request_id: "material-other".into(),
                name: "Solvent".into(),
            },
        )
        .await
        .unwrap();
    // Force the empty material after the funded one in canonical lock order.
    sqlx::query("UPDATE mini_items SET code='ZZ-empty' WHERE code=$1")
        .bind(second["item_code"].as_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let mut multi = consume("rollback", code, "1");
    multi.lines.push(ConsumptionLine {
        item_code: "ZZ-empty".into(),
        percent: "1".into(),
    });
    assert!(matches!(
        store.consume(&actor, multi).await,
        Err(PreparationError::Insufficient)
    ));
    let after: Value = store.snapshot("prep-1").await.unwrap();
    assert_eq!(before["history"], after["history"]);
    assert_eq!(
        before["materials"][0]["balances"],
        after["materials"][0]["balances"]
    );
    assert!(matches!(
        store
            .consume(
                &actor,
                ConsumptionCreate {
                    expected_order_kg: "900".into(),
                    ..consume("stale", code, "1")
                }
            )
            .await,
        Err(PreparationError::Conflict(_))
    ));
    sqlx::query("UPDATE mini_production_maps SET lifecycle_status='cancelled' WHERE id='closed'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        store.consume(&actor, consume("closed", code, "1")).await,
        Err(PreparationError::Conflict(_))
    ));
    sqlx::query("UPDATE mini_raw_material_stock SET status='reserved',reserved_order_id='reserved' WHERE status='available'").execute(&pool).await.unwrap();
    assert!(matches!(
        store.consume(&actor, consume("reserved", code, "1")).await,
        Err(PreparationError::Insufficient)
    ));
    sqlx::query("UPDATE mini_raw_material_stock SET status='available',reserved_order_id='' WHERE status='reserved'").execute(&pool).await.unwrap();
    sqlx::query("UPDATE mini_raw_material_stock SET payload_json=payload_json || '{\"inventory_transfer_id\":\"in-transit\"}'::jsonb WHERE status='available'").execute(&pool).await.unwrap();
    assert!(matches!(
        store.consume(&actor, consume("reserved", code, "1")).await,
        Err(PreparationError::Insufficient)
    ));
    sqlx::query("UPDATE mini_raw_material_stock SET payload_json=payload_json-'inventory_transfer_id' WHERE status='available'").execute(&pool).await.unwrap();
    let (a, b) = tokio::join!(
        store.consume(&actor, consume("race1", code, "3")),
        store.consume(&actor, consume("race2", code, "3"))
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert!(matches!(
        a.err().or(b.err()).unwrap(),
        PreparationError::Insufficient
    ));
    let balance: String =
        sqlx::query_scalar("SELECT sum(qty_delta)::text FROM mini_raw_material_events")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(balance, "10.000000");
    let stock: String = sqlx::query_scalar(
        "SELECT sum(qty)::text FROM mini_raw_material_stock WHERE status='available'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stock, balance);
    assert!(
        sqlx::query("UPDATE mini_preparation_receipts SET initial_kg=1")
            .execute(&pool)
            .await
            .is_err()
    );
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn preparation_child_warehouse_receipt_lands_in_child() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".into());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let db = format!("mini_rs_erp_test_prepchild_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = url.parse::<PgConnectOptions>().unwrap().database(&db);
    let pool = sqlx::PgPool::connect_with(options).await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO mini_system_users(id,role,name,phone) VALUES
            ('prep-1','tayyorlov_masteri','Master','901234567');
        INSERT INTO mini_warehouses(id,name) VALUES ('prep-w','Preparation W');
        INSERT INTO mini_warehouse_assignments(assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
            VALUES ('warehouse','Preparation W','Preparation W','tayyorlov_masteri','prep-1');")
        .execute(&pool).await.unwrap();
    let actor = Principal {
        role: PrincipalRole::TayyorlovMasteri,
        display_name: "Master".into(),
        legal_name: String::new(),
        ref_: "prep-1".into(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let store = PostgresPreparationStore::new(pool.clone());
    // Ota ombor o'zimizniki — tasdiqlanadi; begona ombor — Forbidden.
    assert_eq!(
        store.owned_warehouse_name("prep-1", "Preparation W").await.unwrap(),
        "Preparation W"
    );
    assert!(matches!(
        store.owned_warehouse_name("prep-1", "Other W").await,
        Err(PreparationError::Forbidden)
    ));
    assert!(!store.warehouse_name_exists("Preparation W-1").await.unwrap());
    // Handler shared service lar orqali yaratadigan qatorlar.
    sqlx::raw_sql("INSERT INTO mini_warehouses(id,name,parent_warehouse) VALUES
            ('warehouse:preparation w-1','Preparation W-1','Preparation W');
        INSERT INTO mini_warehouse_assignments(assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
            VALUES ('warehouse','Preparation W-1','Preparation W-1','tayyorlov_masteri','prep-1');")
        .execute(&pool).await.unwrap();
    assert!(store.warehouse_name_exists("preparation w-1").await.unwrap());
    let material = store
        .create_material(
            &actor,
            MaterialCreate {
                request_id: "material-child".into(),
                name: "Kley".into(),
            },
        )
        .await
        .unwrap();
    let code = material["item_code"].as_str().unwrap().to_string();
    // Kirim bola omborga yoziladi va snapshot da shu omborda ko'rinadi.
    let receipt = store
        .receive(
            &actor,
            ReceiptCreate {
                request_id: "receipt-child".into(),
                item_code: code.clone(),
                warehouse: "Preparation W-1".into(),
                kg: "25".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(receipt["warehouse"], "Preparation W-1");
    let snapshot: Value = store.snapshot("prep-1").await.unwrap();
    assert!(
        snapshot["warehouses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w == "Preparation W-1")
    );
    let material_snapshot = snapshot["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["item_code"] == code)
        .unwrap();
    assert_eq!(
        material_snapshot["balances"],
        json!([{"warehouse": "Preparation W-1", "kg": "25.000000"}])
    );
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
