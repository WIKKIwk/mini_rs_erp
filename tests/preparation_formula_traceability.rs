use std::sync::Arc;

use mini_rs_erp::core::auth::models::{Principal, PrincipalRole};
use mini_rs_erp::core::gscale::GscaleService;
use mini_rs_erp::core::preparation::*;
use mini_rs_erp::core::rps_batch::{
    RpsBatchPrintRequest, RpsBatchService, RpsBatchStartRequest, RpsBatchStopRequest,
};
use mini_rs_erp::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    canonical_apparatus_service,
};
use mini_rs_erp::db::postgres_gscale_receipt::PostgresGscaleReceiptStore;
use mini_rs_erp::db::postgres_preparation::PostgresPreparationStore;
use mini_rs_erp::db::postgres_rps_batch::PostgresRpsBatchStore;
use serde_json::{Value, json};
use sqlx::postgres::PgConnectOptions;

#[tokio::test]
#[ignore = "requires PostgreSQL maintenance access; creates an isolated database"]
async fn preparation_formula_recipe_survives_edits_and_follows_each_roll_and_history() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".into());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let database = format!(
        "mini_rs_erp_test_formula_trace_{:016x}",
        rand::random::<u64>()
    );
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool =
        sqlx::PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&database))
            .await
            .unwrap();
    let checks = pool.clone();
    let result = tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move { run_formula_checks(checks).await }).await
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

async fn run_formula_checks(pool: sqlx::PgPool) {
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    canonical_apparatus_service(pool.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_system_users(id,role,name,phone) VALUES
            ('prep-trace','tayyorlov_masteri','Master','901234599');
        INSERT INTO mini_warehouses(id,name) VALUES ('trace-w','Trace W');
        INSERT INTO mini_warehouse_assignments(assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
            VALUES ('warehouse','Trace W','Trace W','tayyorlov_masteri','prep-trace');
        INSERT INTO mini_item_groups(name,parent_item_group,is_group)
            VALUES ('Rulon','Homashyo',true),('seriyo','Homashyo',true) ON CONFLICT DO NOTHING;
        INSERT INTO mini_items(code,name,uom,item_group) VALUES
            ('PE PR','PE PR','Kg','Rulon'),('PET','PET','Kg','Rulon'),
            ('TRACE-S1','Seriya 1','Kg','seriyo'),('TRACE-S2','Seriya 2','Kg','seriyo')
            ON CONFLICT(code) DO UPDATE SET name=excluded.name,item_group=excluded.item_group,uom=excluded.uom;
        INSERT INTO mini_preparation_materials(item_code,owner_ref,name_key) VALUES
            ('TRACE-S1','prep-trace','seriya 1'),('TRACE-S2','prep-trace','seriya 2');
        INSERT INTO mini_preparation_material_responsibilities(principal_role,principal_ref,material_id,material_name)
            VALUES ('tayyorlov_masteri','prep-trace','pe-pr','PE PR');
        INSERT INTO mini_production_maps(id,product_code,title,code,map_json)
            VALUES ('zakaz-trace','P-TRACE','Trace order','0317','{"id":"zakaz-trace","product_code":"P-TRACE","order_kg":300}'),
                   ('zakaz-other','OTHER','Other product','0318','{"id":"zakaz-other","product_code":"OTHER","order_kg":300}');
        INSERT INTO mini_orders(id,code,order_number,product_name)
            VALUES ('zakaz-trace','0317','0317','Trace order'),('zakaz-other','0318','0318','Other product');
        INSERT INTO mini_order_products(id,order_id,product_name,layers_json) VALUES
            ('trace-product','zakaz-trace','P-TRACE','[{"material_id":"pe-pr","material":"PE PR"}]'),
            ('other-product','zakaz-other','OTHER','[{"material_id":"pe-pr","material":"PE PR"}]');
    "#).execute(&pool).await.unwrap();
    let actor = Principal {
        role: PrincipalRole::TayyorlovMasteri,
        ref_: "prep-trace".into(),
        display_name: "Master".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let store = PostgresPreparationStore::new(pool.clone());
    let variant = store
        .create_formula_variant(
            &actor.ref_,
            FormulaVariantCreate {
                product_code: "P-TRACE".into(),
                material_id: "pe-pr".into(),
                name: "LG".into(),
                request_id: "trace-variant-create".into(),
            },
        )
        .await
        .unwrap();
    let variant_id = variant["id"].as_str().unwrap();
    let formula = |name: &str, first: &str, second: &str| FormulaUpsert {
        product_code: "P-TRACE".into(),
        material_id: "pe-pr".into(),
        name: name.into(),
        lines: vec![
            FormulaLine {
                item_code: "TRACE-S1".into(),
                percent: first.into(),
            },
            FormulaLine {
                item_code: "TRACE-S2".into(),
                percent: second.into(),
            },
        ],
    };
    store
        .upsert_formula_variant(&actor, formula("A", "88", "12"), variant_id)
        .await
        .unwrap();
    store
        .upsert_formula_variant(&actor, formula("B", "75", "25"), variant_id)
        .await
        .unwrap();
    let reference = PreparationFormulaReference {
        material_id: "pe-pr".into(),
        variant_id: variant_id.into(),
    };
    assert!(
        store
            .formula_print_snapshot(&actor.ref_, "zakaz-trace", "PE PR", &reference)
            .await
            .is_err(),
        "incomplete screw shares cannot claim a production recipe"
    );
    store
        .update_formula_variant_shares(
            &actor.ref_,
            FormulaSharesUpdate {
                product_code: "P-TRACE".into(),
                material_id: "pe-pr".into(),
                shares: vec![
                    FormulaShare {
                        name: "A".into(),
                        percent: "30".into(),
                    },
                    FormulaShare {
                        name: "B".into(),
                        percent: "70".into(),
                    },
                ],
            },
            false,
            variant_id,
        )
        .await
        .unwrap();
    let snapshot = store
        .formula_print_snapshot(&actor.ref_, "zakaz-trace", "PE PR", &reference)
        .await
        .unwrap();
    assert_eq!(snapshot["variant_name"], "LG");
    assert_eq!(snapshot["formulas"][0]["share_percent"], "30.000000");
    assert_eq!(snapshot["formulas"][0]["lines"][0]["percent"], "88.000000");
    for (owner, order, item) in [
        ("another-master", "zakaz-trace", "PE PR"),
        ("prep-trace", "zakaz-other", "PE PR"),
        ("prep-trace", "zakaz-trace", "PET"),
    ] {
        assert!(
            store
                .formula_print_snapshot(owner, order, item, &reference)
                .await
                .is_err()
        );
    }
    let batches = RpsBatchService::new(Arc::new(PostgresRpsBatchStore::new(pool.clone())));
    let started = batches
        .start(
            &actor,
            RpsBatchStartRequest {
                client_batch_id: "trace-batch".into(),
                driver_url: "usb://local".into(),
                item_code: "PE PR".into(),
                item_name: "PE PR".into(),
                warehouse: "Trace W".into(),
                length_m: Some(125.0),
                printer: "godex".into(),
                order_assignment: Some(
                    json!({"order_id":"zakaz-trace","apparatus":"apparatus:default:asset-005",
            "item_code":"PE PR","item_group":"Rulon","assigned_by_role":"tayyorlov_masteri",
            "assigned_by_ref":actor.ref_,"preparation_formula":snapshot}),
                ),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    store
        .upsert_formula_variant(&actor, formula("A", "50", "50"), variant_id)
        .await
        .unwrap();
    store
        .delete_saved_formula_variant(&actor.ref_, "P-TRACE", "B", "pe-pr", variant_id)
        .await
        .unwrap();
    let (_, mut print, guard) = batches
        .material_receipt_request(
            &actor,
            RpsBatchPrintRequest {
                batch_id: started.batch.id.clone(),
                expected_revision: started.batch.revision,
                expected_item_code: "PE PR".into(),
                expected_warehouse: "Trace W".into(),
                gross_qty: 23.0,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    print.actor_role = "tayyorlov_masteri".into();
    print.actor_ref = actor.ref_.clone();
    let printer = GscaleService::new()
        .with_receipt_store(Arc::new(PostgresGscaleReceiptStore::new(pool.clone())));
    let prepared = printer
        .prepare_material_receipt_client_print(print.clone())
        .unwrap();
    let receipt = printer
        .confirm_material_receipt_client_print(print.clone(), &prepared.epc)
        .await
        .unwrap();
    batches
        .record_print(&actor, &started.batch.id, &receipt)
        .await
        .unwrap();
    assert!(
        printer
            .confirm_material_receipt_client_print(print.clone(), &prepared.epc)
            .await
            .is_ok(),
        "same QR retry is idempotent"
    );
    print.order_assignment.as_mut().unwrap()["preparation_formula"]["variant_name"] =
        json!("Other recipe");
    assert!(
        printer
            .confirm_material_receipt_client_print(print, &prepared.epc)
            .await
            .is_err(),
        "a printed QR cannot acquire another recipe"
    );
    drop(guard);
    for sql in [
        "SELECT payload_json->'order_assignment'->'preparation_formula' FROM mini_gscale_receipts WHERE barcode=$1",
        "SELECT payload_json->'preparation_formula' FROM mini_raw_material_stock WHERE barcode=$1",
        "SELECT payload_json->'preparation_formula' FROM mini_raw_material_assignments WHERE barcode=$1",
        "SELECT payload_json->'preparation_formula' FROM mini_raw_material_events WHERE barcode=$1 AND event_type='receipt_posted'",
    ] {
        let saved: Value = sqlx::query_scalar(sql)
            .bind(&receipt.epc)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(saved, snapshot, "roll retains the original recipe: {sql}");
    }
    let active_history = batches.history(&actor, 50).await.unwrap();
    assert_eq!(
        active_history.batches[0].order_assignment.as_ref().unwrap()["preparation_formula"],
        snapshot,
        "receipt fallback history also carries the recipe before the batch is stopped"
    );
    batches
        .stop(
            &actor,
            RpsBatchStopRequest {
                batch_id: started.batch.id.clone(),
                expected_revision: 1,
            },
        )
        .await
        .unwrap();
    let history = RpsBatchService::new(Arc::new(PostgresRpsBatchStore::new(pool.clone())))
        .history(&actor, 50)
        .await
        .unwrap();
    assert_eq!(
        history.batches[0].order_assignment.as_ref().unwrap()["preparation_formula"],
        snapshot
    );
    assert_eq!(history.batches[0].prints[0].epc, receipt.epc);
}
