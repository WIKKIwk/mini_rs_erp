use crate::core::{
    auth::models::{Principal, PrincipalRole},
    preparation::*,
};
use crate::db::{
    postgres::apply_foundation_migration, postgres_preparation::PostgresPreparationStore,
};
use serde_json::{Value, json};
use sqlx::postgres::PgConnectOptions;
use std::collections::BTreeMap;

use super::seed_standard_canonical_apparatus;

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
            VALUES ('warehouse','Preparation W','Preparation W','tayyorlov_masteri','prep-1');
        INSERT INTO mini_calculate_materials(id,lower_name,payload_json) VALUES
            ('test-mat','testmat','{\"id\":\"test-mat\",\"name\":\"TestMat\",\"active\":true}');")
        .execute(&pool).await.unwrap();
    for id in [
        "order1", "order2", "rollback", "race1", "race2", "stale", "closed", "reserved",
    ] {
        sqlx::query("INSERT INTO mini_production_maps(id,product_code,title,code,map_json) VALUES ($1,'P','Test order',$1,$2)")
            .bind(id).bind(json!({"id":id,"order_kg":1000})).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO mini_orders(id,code,order_number,product_name) VALUES ($1,$1,$1,'P')",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO mini_order_products(id,order_id,product_name,layers_json) VALUES ($1,$2,'P',$3)")
            .bind(format!("{id}:product")).bind(id)
            .bind(json!([{"material_id":"test-mat","material":"TestMat","micron":"12"}]))
            .execute(&pool).await.unwrap();
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
    // Javobgarlik biriktirilmaguncha fail-closed: snapshot bo'sh, consume Forbidden.
    assert!(
        store.snapshot("prep-1").await.unwrap()["orders"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let assigned = store
        .assign_responsibility(MaterialResponsibilityAssign {
            principal_ref: "prep-1".into(),
            material_id: "test-mat".into(),
        })
        .await
        .unwrap();
    assert_eq!(assigned["material_name"], "TestMat");
    // Builtin default katalog DB'da bo'lmasa ham biriktiriladi (API list'dagi kabi).
    let builtin = store
        .assign_responsibility(MaterialResponsibilityAssign {
            principal_ref: "prep-1".into(),
            material_id: "builtin-pet".into(),
        })
        .await
        .unwrap();
    assert_eq!(builtin["material_name"], "PET");
    store
        .unassign_responsibility(MaterialResponsibilityDelete {
            principal_ref: "prep-1".into(),
            material_id: "builtin-pet".into(),
        })
        .await
        .unwrap();
    assert!(
        store.snapshot("prep-1").await.unwrap()["orders"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["id"] == "order1")
    );
    // Boshqa master'da biriktirish yo'q — baribir bo'sh.
    assert!(
        store.snapshot("prep-2").await.unwrap()["orders"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let material_input = MaterialCreate {
        request_id: "material-001".into(),
        name: "Kley".into(),
        warehouse: "Preparation W".into(),
    };
    let material = store
        .create_material(&actor, material_input.clone())
        .await
        .unwrap();
    assert_eq!(material["item_group"], "seriyo");
    assert_eq!(
        sqlx::query_as::<_, (String, String, bool)>(
            "SELECT name, parent_item_group, is_group
             FROM mini_item_groups WHERE lower(name) = 'seriyo'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        (
            "seriyo".to_string(),
            "Homashyo".to_string(),
            true,
        )
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT item_group FROM mini_items WHERE code = $1",)
            .bind(material["item_code"].as_str().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap(),
        "seriyo"
    );
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
                    name: " kley ".into(),
                    warehouse: "Preparation W".into(),
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
    let other_snapshot = store.snapshot("prep-2").await.unwrap();
    let shared_material = other_snapshot["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["item_code"] == code)
        .unwrap();
    assert_eq!(shared_material["can_receive"], false);
    let other = Principal {
        ref_: "prep-2".into(),
        ..actor.clone()
    };
    assert!(matches!(
        store.receive(&other, receipt.clone()).await,
        Err(PreparationError::ReceiptRequiresQr)
    ));
    // Roll back an earlier line's stock update and event when a later line fails.
    let second = store
        .create_material(
            &actor,
            MaterialCreate {
                request_id: "material-other".into(),
                name: "Solvent".into(),
                warehouse: "Preparation W".into(),
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
    // Begona homashyoli order: ombor to'g'ri bo'lsa ham scope Forbidden.
    sqlx::query("INSERT INTO mini_production_maps(id,product_code,title,code,map_json) VALUES ('foreign','P','Foreign','foreign',$1)")
        .bind(json!({"id":"foreign","order_kg":1000})).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_orders(id,code,product_name) VALUES ('foreign','foreign','P')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO mini_order_products(id,order_id,product_name,layers_json) VALUES ('foreign:product','foreign','P',$1)")
        .bind(json!([{"material_id":"other-mat","material":"OtherMat","micron":"20"}]))
        .execute(&pool).await.unwrap();
    assert!(matches!(
        store.consume(&actor, consume("foreign", code, "1")).await,
        Err(PreparationError::Forbidden)
    ));
    assert!(
        store.snapshot("prep-1").await.unwrap()["orders"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o["id"] != "foreign")
    );
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
    let other_warehouse_receipt = store
        .receive(
            &actor,
            ReceiptCreate {
                request_id: "receipt-other-w".into(),
                warehouse: "Other W".into(),
                ..receipt.clone()
            },
        )
        .await;
    assert!(matches!(other_warehouse_receipt, Err(PreparationError::ReceiptRequiresQr)));
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn preparation_receipt_reversal_is_append_only_and_guarded() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".into());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let db = format!(
        "mini_rs_erp_test_prep_reversal_{:016x}",
        rand::random::<u64>()
    );
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = url.parse::<PgConnectOptions>().unwrap().database(&db);
    let pool = sqlx::PgPool::connect_with(options).await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    seed_standard_canonical_apparatus(&pool).await;
    sqlx::raw_sql(
        "INSERT INTO mini_system_users(id, role, name, phone)
             VALUES ('prep-reversal', 'tayyorlov_masteri', 'Reversal master', '901234577');
         INSERT INTO mini_warehouses(id, name)
             VALUES ('prep-reversal-w', 'Reversal W');
         INSERT INTO mini_warehouse_assignments(
             assignment_kind, warehouse, warehouse_name, principal_role, principal_ref
         ) VALUES ('warehouse', 'Reversal W', 'Reversal W',
                   'tayyorlov_masteri', 'prep-reversal');",
    )
    .execute(&pool)
    .await
    .unwrap();
    let actor = Principal {
        role: PrincipalRole::TayyorlovMasteri,
        display_name: "Reversal master".into(),
        legal_name: String::new(),
        ref_: "prep-reversal".into(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let store = PostgresPreparationStore::new(pool.clone());
    let material = store
        .create_material(
            &actor,
            MaterialCreate {
                request_id: "reversal-material".into(),
                name: "Reversal Kley".into(),
                warehouse: "Reversal W".into(),
            },
        )
        .await
        .unwrap();
    let item_code = material["item_code"].as_str().unwrap().to_string();
    let receipt = store
        .receive(
            &actor,
            ReceiptCreate {
                request_id: "reversal-receipt".into(),
                item_code: item_code.clone(),
                warehouse: "Reversal W".into(),
                kg: "12.500000".into(),
            },
        )
        .await
        .unwrap();
    let before = store.snapshot("prep-reversal").await.unwrap();
    let original_before = before["history"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == receipt["id"])
        .unwrap();
    assert_eq!(original_before["can_reverse"], true);
    assert_eq!(original_before["reversed"], false);

    let reversal_input = ReceiptReversalCreate {
        request_id: "reversal-operation".into(),
        receipt_id: receipt["id"].as_str().unwrap().into(),
        reason: "  Xato  miqdor  kiritildi  ".into(),
    };
    let reversed = store
        .reverse_receipt(&actor, reversal_input.clone())
        .await
        .unwrap();
    assert_eq!(reversed["kind"], "receipt_reversal");
    assert_eq!(reversed["status"], "cancelled");
    assert_eq!(reversed["kg"], "12.500000");
    assert_eq!(reversed["reason"], "Xato miqdor kiritildi");
    assert_eq!(
        reversed,
        store
            .reverse_receipt(&actor, reversal_input)
            .await
            .unwrap()
    );

    let stock_status: String = sqlx::query_scalar(
        "SELECT status FROM mini_raw_material_stock WHERE source_receipt_id = $1",
    )
    .bind(receipt["id"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stock_status, "deleted");
    let event: (String, String, String) = sqlx::query_as(
        "SELECT event_type, source_type, qty_delta::text
         FROM mini_raw_material_events
         WHERE source_type = 'stock_delete' AND source_id = $1",
    )
    .bind(reversed["id"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        event,
        (
            "stock_deleted".into(),
            "stock_delete".into(),
            "-12.500000".into()
        )
    );

    let after = store.snapshot("prep-reversal").await.unwrap();
    assert!(after["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|material| material["item_code"] == item_code)
        .unwrap()["balances"]
        .as_array()
        .unwrap()
        .is_empty());
    let history = after["history"].as_array().unwrap();
    let original_after = history
        .iter()
        .find(|entry| entry["id"] == receipt["id"])
        .unwrap();
    assert_eq!(original_after["can_reverse"], false);
    assert_eq!(original_after["reversed"], true);
    assert!(history
        .iter()
        .any(|entry| entry["id"] == reversed["id"] && entry["kind"] == "receipt_reversal"));

    let blocked_receipt = store
        .receive(
            &actor,
            ReceiptCreate {
                request_id: "blocked-receipt".into(),
                item_code: item_code.clone(),
                warehouse: "Reversal W".into(),
                kg: "4".into(),
            },
        )
        .await
        .unwrap();
    let blocked_stock: (String, String) = sqlx::query_as(
        "SELECT barcode, id FROM mini_raw_material_stock WHERE source_receipt_id = $1",
    )
    .bind(blocked_receipt["id"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO mini_production_maps(id, product_code, title, code, map_json)
         VALUES ('reversal-order', 'P', 'Reversal order', 'reversal-order', $1)",
    )
    .bind(json!({"order_kg": 100}))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO mini_raw_material_assignments(
             barcode, order_id, apparatus, canonical_apparatus_id,
             item_code, item_group, payload_json
         ) VALUES ($1, 'reversal-order', 'apparatus:default:bosma_7',
                   'apparatus:default:bosma_7', $2, 'seriyo', '{}'::jsonb)",
    )
    .bind(&blocked_stock.0)
    .bind(&item_code)
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        store
            .reverse_receipt(
                &actor,
                ReceiptReversalCreate {
                    request_id: "blocked-reversal".into(),
                    receipt_id: blocked_receipt["id"].as_str().unwrap().into(),
                    reason: "Adashib kiritilgan".into(),
                },
            )
            .await,
        Err(PreparationError::Conflict(_))
    ));
    let blocked_status: String = sqlx::query_scalar(
        "SELECT status FROM mini_raw_material_stock WHERE id = $1",
    )
    .bind(&blocked_stock.1)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(blocked_status, "available");

    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn preparation_snapshot_lists_shared_raw_catalog_and_balances() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".into());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let db = format!(
        "mini_rs_erp_test_prep_snapshot_{:016x}",
        rand::random::<u64>()
    );
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = url.parse::<PgConnectOptions>().unwrap().database(&db);
    let pool = sqlx::PgPool::connect_with(options).await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO mini_system_users(id,role,name,phone)
             VALUES ('prep-1','tayyorlov_masteri','Master','901234567');
         INSERT INTO mini_warehouses(id,name)
             VALUES ('prep-w','Preparation W'),('other-w','Other W');
         INSERT INTO mini_warehouse_assignments(
             assignment_kind,warehouse,warehouse_name,principal_role,principal_ref
         ) VALUES ('warehouse','Preparation W','Preparation W',
                   'tayyorlov_masteri','prep-1');",
    )
    .execute(&pool)
    .await
    .unwrap();
    let raw_group: String = sqlx::query_scalar(
        "SELECT name FROM mini_item_groups
         WHERE lower(name) = 'homashyo' AND is_group
         ORDER BY name LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO mini_item_groups(name,parent_item_group,is_group)
         VALUES ('Snapshot Raw Group',$1,true)",
    )
    .bind(&raw_group)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(
        "INSERT INTO mini_items(code,name,uom,item_group)
             VALUES ('RAW-SHARED','Shared Raw','Kg','Snapshot Raw Group'),
                    ('RAW-ZERO','Zero Raw','Kg','Snapshot Raw Group');
         INSERT INTO mini_raw_material_stock(
             id,warehouse,item_code,item_name,barcode,qty
         ) VALUES ('raw:snapshot-shared','Other W','RAW-SHARED','Shared Raw',
                   'SNAPSHOT-SHARED','17');",
    )
    .execute(&pool)
    .await
    .unwrap();

    let roll_group: String = sqlx::query_scalar(
        "SELECT name FROM mini_item_groups
         WHERE lower(name) = 'rulon' AND is_group
         ORDER BY name LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO mini_item_groups(name,parent_item_group,is_group)
         SELECT 'seriyo',$1,true
         WHERE NOT EXISTS (
             SELECT 1 FROM mini_item_groups WHERE lower(name) = 'seriyo'
         )",
    )
    .bind(&raw_group)
    .execute(&pool)
    .await
    .unwrap();
    let seriyo_group: String = sqlx::query_scalar(
        "SELECT name FROM mini_item_groups
         WHERE lower(name) = 'seriyo' AND is_group
         ORDER BY name LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO mini_calculate_materials(id,lower_name,payload_json)
         VALUES ('snapshot-mat','snapshotassigned',
                 '{\"id\":\"snapshot-mat\",\"name\":\"Snapshot Assigned\",\"active\":true}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO mini_items(code,name,uom,item_group)
         VALUES ('SNAP-RULON','Snapshot Assigned','Kg',$1),
                ('SNAP-SERIYO','Snapshot Assigned','Kg',$2),
                ('SNAP-OTHER','Snapshot Assigned','Kg','Snapshot Raw Group')",
    )
    .bind(&roll_group)
    .bind(&seriyo_group)
    .execute(&pool)
    .await
    .unwrap();

    let store = PostgresPreparationStore::new(pool.clone());
    store
        .assign_responsibility(MaterialResponsibilityAssign {
            principal_ref: "prep-1".into(),
            material_id: "snapshot-mat".into(),
        })
        .await
        .unwrap();
    let assigned_rulon = store
        .assigned_rulon_items("prep-1", "", 50, 0)
        .await
        .unwrap();
    assert_eq!(
        assigned_rulon
            .iter()
            .map(|item| item.code.as_str())
            .collect::<Vec<_>>(),
        vec!["SNAP-RULON"]
    );
    let simple = store
        .receive_gscale_simple(
            &Principal {
                role: PrincipalRole::TayyorlovMasteri,
                display_name: "Master".into(),
                legal_name: String::new(),
                ref_: "prep-1".into(),
                phone: String::new(),
                avatar_url: String::new(),
            },
            ReceiptCreate {
                request_id: "gscale-simple-001".into(),
                item_code: "SNAP-RULON".into(),
                warehouse: "Preparation W".into(),
                kg: "12.5".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(simple["qr_printed"], false);
    assert_eq!(simple["item_code"], "SNAP-RULON");
    assert_eq!(simple["kg"], "12.500000");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM mini_preparation_receipts WHERE item_code = 'SNAP-RULON'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );

    let snapshot = store.snapshot("prep-1").await.unwrap();
    let materials = snapshot["materials"].as_array().unwrap();
    let shared = materials
        .iter()
        .find(|item| item["item_code"] == "RAW-SHARED")
        .unwrap();
    assert_eq!(shared["name"], "Shared Raw");
    assert_eq!(shared["can_receive"], false);
    assert_eq!(
        shared["balances"],
        json!([{"warehouse": "Other W", "kg": "17.000000"}])
    );
    let zero = materials
        .iter()
        .find(|item| item["item_code"] == "RAW-ZERO")
        .unwrap();
    assert_eq!(zero["balances"], json!([]));

    let scopes = BTreeMap::from([(
        "Other W".to_string(),
        PreparationWarehouseMaterialScope::AssignedItemGroups(vec![
            "Snapshot Raw Group".to_string(),
        ]),
    )]);
    let scoped = PostgresPreparationStore::new(pool.clone())
        .snapshot_with_warehouse_material_scopes("prep-1", &scopes)
        .await
        .unwrap();
    assert_eq!(
        scoped["materials"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["item_code"] == "RAW-SHARED")
            .unwrap()["visible_warehouses"],
        json!(["Other W"])
    );
    assert_eq!(
        scoped["materials"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["item_code"] == "RAW-ZERO")
            .unwrap()["visible_warehouses"],
        json!(["Other W"])
    );

    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn preparation_formula_material_scope_and_order_materials() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".into());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let db = format!(
        "mini_rs_erp_test_prepformula_{:016x}",
        rand::random::<u64>()
    );
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
            VALUES ('warehouse','Preparation W','Preparation W','tayyorlov_masteri','prep-1');
        INSERT INTO mini_calculate_materials(id,lower_name,payload_json) VALUES
            ('test-mat','testmat','{\"id\":\"test-mat\",\"name\":\"TestMat\",\"active\":true}'),
            ('other-mat','othermat','{\"id\":\"other-mat\",\"name\":\"OtherMat\",\"active\":true}');
        INSERT INTO mini_production_maps(id,product_code,title,code,map_json)
            VALUES ('order1','P','Test order','order1','{\"id\":\"order1\",\"order_kg\":1000}');
        INSERT INTO mini_orders(id,code,order_number,product_name)
            VALUES ('order1','order1','order1','P');
        INSERT INTO mini_order_products(id,order_id,product_name,layers_json)
            VALUES ('order1:product','order1','P','[{\"material_id\":\"test-mat\",\"material\":\"TestMat\",\"micron\":\"12\"}]');")
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
    store
        .assign_responsibility(MaterialResponsibilityAssign {
            principal_ref: "prep-1".into(),
            material_id: "test-mat".into(),
        })
        .await
        .unwrap();
    // Order qatlamlari faqat biriktirilgan homashyoni ko'rsatadi.
    let order_materials = store.order_materials("order1").await.unwrap();
    assert_eq!(
        order_materials["materials"],
        json!([{"material_id": "test-mat", "material_name": "TestMat"}])
    );
    // Seriya uchun PREP material kerak.
    let material = store
        .create_material(
            &actor,
            MaterialCreate {
                request_id: "material-f1".into(),
                name: "Kley".into(),
                warehouse: "Preparation W".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(material["item_group"], "seriyo");
    let code = material["item_code"].as_str().unwrap().to_string();
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT warehouse_name FROM mini_preparation_materials WHERE item_code=$1",
        )
        .bind(&code)
        .fetch_one(&pool)
        .await
        .unwrap(),
        "Preparation W"
    );
    sqlx::query(
        "INSERT INTO mini_item_groups(name,parent_item_group,is_group)
         VALUES ('Non Seriyo',$1,true)",
    )
    .bind("Homashyo")
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO mini_items(code,name,uom,item_group)
         VALUES ('PREP-NON-SERIYO','Non Seriyo item','kg','Non Seriyo')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO mini_preparation_materials(item_code,owner_ref,name_key)
         VALUES ('PREP-NON-SERIYO','prep-1','non seriyo item')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let owned_seriyo = store
        .owned_seriyo_items("prep-1", "", 50, 0)
        .await
        .unwrap();
    assert!(owned_seriyo.iter().any(|item| item.code == code));
    assert!(!owned_seriyo
        .iter()
        .any(|item| item.code == "PREP-NON-SERIYO"));
    let snapshot_material = store
        .snapshot("prep-1")
        .await
        .unwrap()["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["item_code"] == "PREP-NON-SERIYO")
        .cloned()
        .unwrap();
    assert_eq!(snapshot_material["can_receive"], false);
    let own_scope = BTreeMap::from([(
        "Preparation W".to_string(),
        PreparationWarehouseMaterialScope::OwnSeriyo,
    )]);
    let scoped = store
        .snapshot_with_warehouse_material_scopes("prep-1", &own_scope)
        .await
        .unwrap();
    let own_material = scoped["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["item_code"] == code)
        .unwrap();
    assert_eq!(own_material["visible_warehouses"], json!(["Preparation W"]));
    assert_eq!(snapshot_material["visible_warehouses"], Value::Null);
    let formula_input = |material_id: &str| FormulaUpsert {
        product_code: "PC-1".into(),
        name: "Asosiy".into(),
        material_id: material_id.into(),
        lines: vec![FormulaLine {
            item_code: code.clone(),
            percent: "100".into(),
        }],
    };
    // Biriktirilmagan homashyoga yozish taqiqlanadi.
    assert!(matches!(
        store
            .upsert_formula(&actor, formula_input("other-mat"))
            .await,
        Err(PreparationError::Forbidden)
    ));
    let saved = store
        .upsert_formula(&actor, formula_input("test-mat"))
        .await
        .unwrap();
    assert_eq!(saved["material_id"], "test-mat");
    assert_eq!(saved["material_name"], "TestMat");
    let listed = store
        .list_formulas("prep-1", "PC-1", "test-mat")
        .await
        .unwrap();
    assert_eq!(listed["formulas"].as_array().unwrap().len(), 1);
    // Biriktirilmagan homashyo ro'yxati bo'sh (fail-closed).
    let foreign = store
        .list_formulas("prep-1", "PC-1", "other-mat")
        .await
        .unwrap();
    assert!(foreign["formulas"].as_array().unwrap().is_empty());
    assert!(matches!(
        store
            .delete_formula("prep-1", "PC-1", "Asosiy", "other-mat")
            .await,
        Err(PreparationError::Forbidden)
    ));
    let deleted = store
        .delete_formula("prep-1", "PC-1", "Asosiy", "test-mat")
        .await
        .unwrap();
    assert_eq!(deleted["deleted"], true);
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
            VALUES ('warehouse','Preparation W','Preparation W','tayyorlov_masteri','prep-1'),
                   ('warehouse','Preparation W','Preparation W','material_taminotchi','mt-1');")
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
        store
            .owned_warehouse_name("prep-1", "Preparation W")
            .await
            .unwrap(),
        "Preparation W"
    );
    assert!(matches!(
        store.owned_warehouse_name("prep-1", "Other W").await,
        Err(PreparationError::Forbidden)
    ));
    assert!(
        !store
            .warehouse_name_exists("Preparation W-1")
            .await
            .unwrap()
    );
    // Handler shared service lar orqali yaratadigan qatorlar.
    sqlx::raw_sql("INSERT INTO mini_warehouses(id,name,parent_warehouse) VALUES
            ('warehouse:preparation w-1','Preparation W-1','Preparation W');
        INSERT INTO mini_warehouse_assignments(assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
            VALUES ('warehouse','Preparation W-1','Preparation W-1','tayyorlov_masteri','prep-1');")
        .execute(&pool).await.unwrap();
    assert!(
        store
            .warehouse_name_exists("preparation w-1")
            .await
            .unwrap()
    );
    assert!(matches!(
        store
            .create_material(
                &actor,
                MaterialCreate {
                    request_id: "material-shared-parent".into(),
                    name: "Parent material".into(),
                    warehouse: "Preparation W".into(),
                },
            )
            .await,
        Err(PreparationError::WarehouseNotExclusive)
    ));
    let material = store
        .create_material(
            &actor,
            MaterialCreate {
                request_id: "material-child".into(),
                name: "Kley".into(),
                warehouse: "Preparation W-1".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(material["warehouse"], "Preparation W-1");
    let code = material["item_code"].as_str().unwrap().to_string();
    assert_eq!(
        sqlx::query_as::<_, (String, String, bool)>(
            "SELECT warehouse.id, scope.scope_kind, scope.active
             FROM mini_preparation_material_warehouse_scopes scope
             JOIN mini_warehouses warehouse ON warehouse.id = scope.warehouse_id
             WHERE scope.item_code = $1",
        )
        .bind(&code)
        .fetch_one(&pool)
        .await
        .unwrap(),
        (
            "warehouse:preparation w-1".to_string(),
            "exclusive".to_string(),
            true,
        )
    );
    let scopes = BTreeMap::from([
        (
            "Preparation W".to_string(),
            PreparationWarehouseMaterialScope::OwnSeriyo,
        ),
        (
            "Preparation W-1".to_string(),
            PreparationWarehouseMaterialScope::OwnSeriyo,
        ),
    ]);
    let scoped_snapshot = store
        .snapshot_with_warehouse_material_scopes("prep-1", &scopes)
        .await
        .unwrap();
    let scoped_material = scoped_snapshot["materials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["item_code"] == code)
        .unwrap();
    assert_eq!(
        scoped_material["visible_warehouses"],
        json!(["Preparation W-1"])
    );
    sqlx::raw_sql(
        "INSERT INTO mini_warehouses(id,name,parent_warehouse)
             VALUES ('warehouse:preparation w-2','Preparation W-2','Preparation W');
         INSERT INTO mini_warehouse_assignments(
             assignment_kind,warehouse,warehouse_name,principal_role,principal_ref
         ) VALUES ('warehouse','Preparation W-2','Preparation W-2','tayyorlov_masteri','prep-1');",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        store
            .receive(
                &actor,
                ReceiptCreate {
                    request_id: "receipt-wrong-child".into(),
                    item_code: code.clone(),
                    warehouse: "Preparation W-2".into(),
                    kg: "1".into(),
                }
            )
            .await,
        Err(PreparationError::MaterialNotInWarehouse)
    ));
    sqlx::raw_sql(
        "DELETE FROM mini_warehouse_assignments WHERE warehouse_name='Preparation W-2';
         DELETE FROM mini_warehouses WHERE name='Preparation W-2';",
    )
    .execute(&pool)
    .await
    .unwrap();
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
    sqlx::query(
        "INSERT INTO mini_raw_material_stock
             (id, warehouse, item_code, item_name, barcode, qty)
         VALUES ('raw:prepchild-parent-leak', 'Preparation W', $1, 'Kley',
                 'PREP-CHILD-PARENT-LEAK', 7)",
    )
    .bind(&code)
    .execute(&pool)
    .await
    .unwrap();
    let snapshot: Value = store.snapshot("prep-1").await.unwrap();
    assert!(
        snapshot["warehouses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w == "Preparation W-1")
    );
    assert!(
        snapshot["assigned_warehouses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w == "Preparation W")
    );
    assert_eq!(snapshot["material_warehouses"], json!(["Preparation W-1"]));
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
    let shared_receipt = store
        .receive(
            &actor,
            ReceiptCreate {
                request_id: "receipt-shared-parent".into(),
                item_code: code.clone(),
                warehouse: "Preparation W".into(),
                kg: "5".into(),
            },
        )
        .await;
    assert!(matches!(shared_receipt, Err(PreparationError::ReceiptRequiresQr)));

    // A new assignment after the screen loaded revokes both actions. This
    // includes another role using the same ref, not only a different master.
    for (role, principal_ref) in [
        ("material_taminotchi", "prep-1"),
        ("omborchi", "warehouse-owner"),
        ("tayyorlov_masteri", "prep-2"),
    ] {
        sqlx::query("INSERT INTO mini_warehouse_assignments
            (assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
            VALUES ('warehouse','Preparation W-1','Preparation W-1',$1,$2)")
            .bind(role).bind(principal_ref).execute(&pool).await.unwrap();
        assert_eq!(store.snapshot("prep-1").await.unwrap()["material_warehouses"], json!([]));
        assert!(matches!(store.receive(&actor, ReceiptCreate {
            request_id: format!("revoked-receipt-{role}"),
            item_code: code.clone(), warehouse: "Preparation W-1".into(), kg: "1".into(),
        }).await, Err(PreparationError::ReceiptRequiresQr)));
        assert!(matches!(store.create_material(&actor, MaterialCreate {
            request_id: format!("revoked-material-{role}"),
            name: "Not allowed".into(), warehouse: "Preparation W-1".into(),
        }).await, Err(PreparationError::WarehouseNotExclusive)));
        sqlx::query("DELETE FROM mini_warehouse_assignments WHERE warehouse_name='Preparation W-1'
            AND principal_role=$1 AND principal_ref=$2")
            .bind(role).bind(principal_ref).execute(&pool).await.unwrap();
    }
    assert_eq!(store.snapshot("prep-1").await.unwrap()["material_warehouses"], json!(["Preparation W-1"]));
    // A sharing transaction already in flight must win before the manual
    // receipt checks exclusivity; no receipt may use an older assignment view.
    let mut sharing = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO mini_warehouse_assignments
        (assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
        VALUES ('warehouse','Preparation W-1','Preparation W-1','material_taminotchi','mt-race')")
        .execute(&mut *sharing).await.unwrap();
    let race_store = store.clone();
    let mut receiving = tokio::spawn(async move {
        race_store.receive(&actor, ReceiptCreate {
            request_id: "receipt-assignment-race".into(),
            item_code: code, warehouse: "Preparation W-1".into(), kg: "1".into(),
        }).await
    });
    assert!(tokio::time::timeout(std::time::Duration::from_millis(50), &mut receiving).await.is_err());
    sharing.commit().await.unwrap();
    assert!(matches!(receiving.await.unwrap(), Err(PreparationError::ReceiptRequiresQr)));
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mini_preparation_receipts")
        .fetch_one(&pool).await.unwrap(), 1);
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
