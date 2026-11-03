use std::sync::Arc;

use mini_rs_erp::core::auth::models::{Principal, PrincipalRole};
use mini_rs_erp::core::gscale::GscaleService;
use mini_rs_erp::core::preparation::*;
use mini_rs_erp::core::rps_batch::models::RpsBatchSession;
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
    apply_postgres_migrations_through_version(&pool, "0187").await.unwrap();
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
    sqlx::raw_sql(r#"
        INSERT INTO mini_orders(id,code,order_number,product_name)
            VALUES ('zakaz-second','0319','0319','Trace order');
        INSERT INTO mini_production_maps(id,product_code,title,code,map_json)
            VALUES ('zakaz-second','P-TRACE','Trace order','0319','{}');
        INSERT INTO mini_order_products(id,order_id,product_name,layers_json)
            VALUES ('trace-second','zakaz-second','P-TRACE','[{"material_id":"pe-pr"}]');
        INSERT INTO mini_preparation_formula_variants(id,owner_ref,product_code,material_id,name)
            VALUES ('fv-legacy-trace','prep-trace','P-TRACE','pe-pr','Catalog legacy');
    "#).execute(&pool).await.unwrap();
    let legacy_batch = RpsBatchSession {
        id: "legacy".into(),
        order_assignment: Some(json!({
            "order_id": "zakaz-second",
            "preparation_formula": {
                "schema_version":1,"variant_id":"fv-legacy-trace","variant_name":"Old name",
                "owner_ref":"prep-trace","product_code":"P-TRACE","material_id":"pe-pr",
                "captured_at":"2026-01-01T00:00:00Z",
                "formulas":[{"name":"A","share_percent":"100.000000","lines":[
                    {"item_code":"TRACE-S1","name":"Seriya 1","percent":"100.000000"}
                ]}]
            }
        })),
        ..RpsBatchSession::inactive("tayyorlov_masteri:prep-trace".into(),
            "tayyorlov_masteri".into(), "prep-trace".into())
    };
    sqlx::query(
        "INSERT INTO mini_rps_batch_history(batch_id,owner_key,owner_role,owner_ref,payload_json,completed_at)
         VALUES ($1,$2,$3,$4,$5,'2026-01-01T00:00:00Z')",
    )
    .bind(&legacy_batch.id)
    .bind(&legacy_batch.owner_key)
    .bind(&legacy_batch.owner_role)
    .bind(&legacy_batch.owner_ref)
    .bind(serde_json::to_value(&legacy_batch).unwrap())
    .execute(&pool)
    .await
    .unwrap();
    apply_postgres_migrations_through_version(&pool, "0188").await.unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_preparation_formula_variants
            (id,owner_ref,product_code,material_id,name,request_id,request_name)
        VALUES ('fv-recover','prep-trace','P-TRACE','pe-pr','Reusable','old-request','Reusable'),
               ('fv-cleared','prep-trace','P-TRACE','pe-pr','Cleared','cleared-request','Cleared');
        INSERT INTO mini_preparation_formula_revisions(variant_id,content_key,formulas)
        SELECT id, mini_preparation_recipe_key('[]'), '[]'
        FROM mini_preparation_formula_variants WHERE id IN ('fv-recover','fv-cleared');

        WITH recipe AS (
            SELECT id, jsonb_agg(jsonb_build_object(
                'variant_id', id, 'formula_code', mini_preparation_next_formula_code(),
                'set_id', CASE WHEN id='fv-cleared' THEN '950200000001' ELSE NULL END,
                'name', part.name, 'share_percent', part.share,
                'lines', '[{"item_code":"TRACE-S1","name":"Seriya 1","percent":"100.000000"}]'::jsonb
            ) ORDER BY part.name) AS formulas
            FROM mini_preparation_formula_variants
            CROSS JOIN (VALUES ('A','10.000000'),('B','90.000000')) part(name,share)
            WHERE id IN ('fv-recover','fv-cleared') GROUP BY id
        ), revisions AS (
            INSERT INTO mini_preparation_formula_revisions(variant_id,content_key,formulas)
            SELECT id, mini_preparation_recipe_key(formulas), formulas FROM recipe
            RETURNING variant_id, id
        )
        INSERT INTO mini_preparation_order_formula_selections
            (owner_ref,order_id,variant_id,product_code,material_id,revision_id,display_name)
        SELECT 'prep-trace','zakaz-second',variant_id,'P-TRACE','pe-pr',id,'Local name' FROM revisions;
    "#).execute(&pool).await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    let actor = Principal {
        role: PrincipalRole::TayyorlovMasteri,
        ref_: "prep-trace".into(),
        display_name: "Master".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let store = PostgresPreparationStore::new(pool.clone());
    let legacy = store.list_order_formula_components(
        &actor.ref_, "P-TRACE", "pe-pr", "fv-legacy-trace", "zakaz-second",
    ).await.unwrap();
    assert_eq!(legacy["variant_name"], "Old name");
    assert_eq!(legacy["formulas"][0]["lines"][0]["percent"], "100.000000");

    let recovered = store.list_saved_formulas_variant(
        &actor.ref_, "P-TRACE", "pe-pr", "fv-recover",
    ).await.unwrap();
    let local = store.list_order_formula_components(
        &actor.ref_, "P-TRACE", "pe-pr", "fv-recover", "zakaz-second",
    ).await.unwrap();
    assert_eq!(local["variant_name"], "Local name");
    assert_eq!(recovered["formulas"].as_array().unwrap().len(), 2);
    for (saved, original) in recovered["formulas"].as_array().unwrap().iter()
        .zip(local["formulas"].as_array().unwrap()) {
        assert_eq!(saved["formula_code"], original["formula_code"]);
        assert_eq!(saved["lines"], original["lines"]);
        assert_eq!(saved["share_percent"], original["share_percent"]);
    }
    let cleared = store.list_saved_formulas_variant(
        &actor.ref_, "P-TRACE", "pe-pr", "fv-cleared",
    ).await.unwrap();
    assert_eq!(cleared["formulas"], json!([]));

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
    store.remove_formula_variant_from_order(&actor.ref_, FormulaVariantOrderRemove {
        order_id: "zakaz-trace".into(), product_code: "P-TRACE".into(),
        material_id: "pe-pr".into(), variant_id: variant_id.into(),
    }).await.unwrap();
    assert!(store.formula_print_snapshot(&actor.ref_, "zakaz-trace", "PE PR", &reference).await.is_err());
    store.reuse_formula_variant_in_order(&actor.ref_, FormulaVariantReuse {
        product_code: "P-TRACE".into(), material_id: "pe-pr".into(), source_variant_id: variant_id.into(),
    }, "zakaz-trace").await.unwrap();
    assert!(store.formula_print_snapshot(&actor.ref_, "zakaz-trace", "PE PR", &reference).await.is_ok());
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

    let target = |order: &str| FormulaVariantTarget {
        order_id: order.into(),
        product_code: "P-TRACE".into(),
        material_id: "pe-pr".into(),
        variant_id: variant_id.into(),
        recipe_key: String::new(),
    };
    let first = store.rename_formula_variant(&actor.ref_, target("zakaz-trace"), "  Buyurtma   LG ")
        .await.unwrap();
    assert_eq!(first["id"], variant["id"]);
    assert_eq!(first["formula_code"], variant["formula_code"]);
    assert_eq!(first["revision_id"], snapshot["revision_id"]);
    assert_eq!(first["name"], "Buyurtma LG");
    assert!(matches!(
        store.rename_formula_variant(&actor.ref_, target("zakaz-trace"), "catalog LEGACY").await,
        Err(PreparationError::Conflict(_))
    ));
    assert!(store.rename_formula_variant("another-master", target("zakaz-trace"), "Foreign").await.is_err());
    assert!(store.rename_formula_variant(&actor.ref_, target("zakaz-other"), "Foreign").await.is_err());

    store.rename_formula_variant(&actor.ref_, target(""), "Catalog LG").await.unwrap();
    let replay = store.create_formula_variant(&actor.ref_, FormulaVariantCreate {
        product_code: "P-TRACE".into(), material_id: "pe-pr".into(),
        name: "LG".into(), request_id: "trace-variant-create".into(),
    }).await.unwrap();
    assert_eq!(replay["id"], variant["id"]);

    let first_read = store.list_order_formula_components(
        &actor.ref_, "P-TRACE", "pe-pr", variant_id, "zakaz-trace",
    ).await.unwrap();
    assert_eq!(first_read["variant_name"], "Buyurtma LG");
    assert_eq!(first_read["formulas"], snapshot["formulas"]);
    let mut stale = target("zakaz-second");
    stale.recipe_key = first_read["recipe_key"].as_str().unwrap().into();
    assert!(matches!(store.select_formula_variant(&actor.ref_, stale).await,
        Err(PreparationError::Conflict(_))));
    let second = store.select_formula_variant(&actor.ref_, target("zakaz-second")).await.unwrap();
    assert_eq!(second["name"], "Catalog LG");
    assert_ne!(second["revision_id"], first["revision_id"]);
    assert_eq!(second["formula_count"], 1);

    let mut same = formula("A", "88.0", "12.000000");
    same.lines.reverse();
    store.upsert_formula_in_order(&actor, same, variant_id, "zakaz-trace").await.unwrap();
    let no_change = store.select_formula_variant(&actor.ref_, target("zakaz-trace")).await.unwrap();
    assert_eq!(no_change["revision_id"], first["revision_id"]);

    store.upsert_formula_in_order(&actor, formula("A", "70", "30"), variant_id, "zakaz-trace").await.unwrap();
    let changed = store.formula_print_snapshot(&actor.ref_, "zakaz-trace", "PE PR", &reference).await.unwrap();
    assert_ne!(changed["revision_id"], first["revision_id"]);
    assert_eq!(changed["variant_name"], "Buyurtma LG");
    assert_eq!(changed["formulas"][0]["lines"][0]["percent"], "70.000000");
    let other = store.list_order_formula_components(
        &actor.ref_, "P-TRACE", "pe-pr", variant_id, "zakaz-second",
    ).await.unwrap();
    assert_eq!(other["formulas"][0]["lines"][0]["percent"], "50.000000");
    assert_eq!(other["variant_name"], "Catalog LG");

}
