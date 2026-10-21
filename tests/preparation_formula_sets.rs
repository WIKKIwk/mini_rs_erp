use mini_rs_erp::core::{
    auth::models::{Principal, PrincipalRole},
    preparation::*,
};
use mini_rs_erp::db::{
    postgres::{apply_postgres_migrations_through_version, canonical_apparatus_service},
    postgres_preparation::PostgresPreparationStore,
};
use serde_json::json;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

fn actor(owner: &str) -> Principal {
    Principal {
        role: PrincipalRole::TayyorlovMasteri,
        ref_: owner.into(),
        display_name: "Master".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}

fn formula(product: &str, name: &str, material: &str) -> FormulaUpsert {
    FormulaUpsert {
        product_code: product.into(),
        material_id: "mat-a".into(),
        name: name.into(),
        lines: vec![FormulaLine {
            item_code: material.into(),
            percent: "100".into(),
        }],
    }
}

fn rename(id: &str, name: &str) -> FormulaSetRename {
    FormulaSetRename {
        set_id: id.into(),
        name: name.into(),
    }
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn formula_sets_backfill_identity_names_scopes_and_concurrent_creation() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let database = format!("mini_test_formula_sets_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = url.parse::<PgConnectOptions>().unwrap().database(&database);
    let pool = PgPool::connect_with(options.clone()).await.unwrap();
    let checks = pool.clone();
    let result = tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move { run_checks(&checks, options).await }).await
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

async fn run_checks(pool: &PgPool, options: PgConnectOptions) {
    apply_postgres_migrations_through_version(pool, "0121")
        .await
        .unwrap();
    canonical_apparatus_service(pool.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    apply_postgres_migrations_through_version(pool, "0128")
        .await
        .unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_system_users(id,role,name,phone) VALUES
          ('prep-1','tayyorlov_masteri','Master','901234567'),
          ('prep-2','tayyorlov_masteri','Other','901234568');
        INSERT INTO mini_warehouses(id,name) VALUES ('own','Own');
        INSERT INTO mini_warehouse_assignments(assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
          VALUES ('warehouse','Own','Own','tayyorlov_masteri','prep-1');
        INSERT INTO mini_calculate_materials(id,lower_name,payload_json)
          VALUES ('mat-a','mat a','{"id":"mat-a","name":"Mat A","active":true}');
        INSERT INTO mini_preparation_material_responsibilities(principal_role,principal_ref,material_id,material_name)
          VALUES ('tayyorlov_masteri','prep-1','mat-a','Mat A'),
                 ('tayyorlov_masteri','prep-2','mat-a','Mat A');
        INSERT INTO mini_orders(id,code,order_number,product_name)
          VALUES ('o1','0261','0261','Classic qurt 7 gr'),('o2','0262','0262','Classic qurt 7 gr');
        INSERT INTO mini_production_maps(id,product_code,title,code,map_json) VALUES
          ('o1','P','Classic qurt 7 gr','0261','{}'),('o2','P','Classic qurt 7 gr','0262','{}'),
          ('template-o1','P','Classic qurt 7 gr','','{}');
        INSERT INTO mini_order_products(id,order_id,product_name,layers_json) VALUES
          ('p1','o1','Classic qurt 7 gr','[{"material_id":"mat-a"}]'),
          ('p2','o2','Classic qurt 7 gr','[{"material_id":"mat-a"}]');
        INSERT INTO mini_quick_order_templates(id,owner_key,code,name,item_code,product_name,quick_key,payload_json)
          VALUES ('t1','test','T1','Template','P','Classic qurt 7 gr','template',
                  '{"source_map_id":"template-o1","layers":[{"material_id":"mat-a"}]}');
        INSERT INTO mini_preparation_formulas(owner_ref,product_code,material_id,material_name,name,lines)
          VALUES ('prep-1','P','mat-a','Mat A','A','[]'),
                 ('prep-1','P','','','Legacy','[]'),
                 ('prep-2','P','mat-a','Mat A','Foreign','[]');
    "#).execute(pool).await.unwrap();
    let mut migration = pool.begin().await.unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/postgres/0158_preparation_formula_sets.sql"
    ))
    .execute(&mut *migration)
    .await
    .unwrap();
    migration.commit().await.unwrap();

    let runtime = PgPoolOptions::new()
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .unwrap();
    let store = PostgresPreparationStore::new(runtime.clone());
    let sets = store.formula_sets("prep-1").await.unwrap();
    assert_eq!(sets["sets"].as_array().unwrap().len(), 1);
    let set = &sets["sets"][0];
    let id = set["set_id"].as_str().unwrap().to_owned();
    assert_eq!(id.len(), 12);
    assert!(id.starts_with("9502") && id.bytes().all(|byte| byte.is_ascii_digit()));
    assert_eq!(set["name"], "Classic qurt 7 gr");
    assert_eq!(set["product_title"], "Classic qurt 7 gr");
    assert_eq!(set["order_codes"], json!(["0261", "0262"]));
    assert_eq!(set["formulas"].as_array().unwrap().len(), 2);
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(DISTINCT set_id) FROM mini_preparation_formulas WHERE owner_ref='prep-1' AND product_code='P'")
        .fetch_one(pool).await.unwrap(), 1);
    let foreign = store.formula_sets("prep-2").await.unwrap();
    assert_ne!(foreign["sets"][0]["set_id"], id);
    assert!(
        store.formula_sets("unknown").await.unwrap()["sets"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let changed = store
        .rename_formula_set("prep-1", rename(&id, "  Qurt maxsus  "))
        .await
        .unwrap();
    assert_eq!(changed, json!({"set_id":id,"name":"Qurt maxsus"}));
    for invalid in ["", "   ", &"x".repeat(161)] {
        assert!(
            store
                .rename_formula_set("prep-1", rename(&id, invalid))
                .await
                .is_err()
        );
    }
    assert!(
        store
            .rename_formula_set("prep-1", rename("950100000001", "Wrong type"))
            .await
            .is_err()
    );
    assert!(
        store
            .rename_formula_set("prep-2", rename(&id, "Foreign write"))
            .await
            .is_err()
    );
    assert!(
        store
            .rename_formula_set("unknown", rename(&id, "Foreign write"))
            .await
            .is_err()
    );
    assert!(
        sqlx::query(
            "UPDATE mini_preparation_formula_sets SET set_id='950200999999' WHERE set_id=$1"
        )
        .bind(&id)
        .execute(&runtime)
        .await
        .is_err()
    );

    let material = store
        .create_material(
            &actor("prep-1"),
            MaterialCreate {
                request_id: "formula-set-material".into(),
                name: "0209".into(),
                warehouse: "Own".into(),
            },
        )
        .await
        .unwrap()["item_code"]
        .as_str()
        .unwrap()
        .to_owned();
    store
        .upsert_formula(&actor("prep-1"), formula("P", "A", &material))
        .await
        .unwrap();
    store
        .upsert_formula(&actor("prep-1"), formula("P", "B", &material))
        .await
        .unwrap();
    let sets = store.formula_sets("prep-1").await.unwrap();
    assert_eq!(sets["sets"][0]["set_id"], id);
    assert_eq!(sets["sets"][0]["name"], "Qurt maxsus");
    assert_eq!(sets["sets"][0]["formulas"].as_array().unwrap().len(), 3);

    // The owner can still manage saved formulas after a responsibility is revoked.
    sqlx::query(
        "DELETE FROM mini_preparation_material_responsibilities WHERE principal_ref='prep-1'",
    )
    .execute(pool)
    .await
    .unwrap();
    let saved = store.formula_sets("prep-1").await.unwrap();
    assert_eq!(saved["sets"][0]["set_id"], id);
    assert_eq!(saved["sets"][0]["order_codes"], json!([]));
    assert_eq!(saved["sets"][0]["formulas"].as_array().unwrap().len(), 3);
    store
        .update_saved_formula(&actor("prep-1"), formula("P", "A", &material))
        .await
        .unwrap();
    store
        .rename_formula_set("prep-1", rename(&id, "Qurt saqlangan"))
        .await
        .unwrap();
    for (name, scope) in [("A", "mat-a"), ("B", "mat-a"), ("Legacy", "")] {
        store
            .delete_saved_formula("prep-1", "P", name, scope)
            .await
            .unwrap();
    }
    assert!(
        store.formula_sets("prep-1").await.unwrap()["sets"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    sqlx::query("INSERT INTO mini_preparation_material_responsibilities(principal_role,principal_ref,material_id,material_name) VALUES ('tayyorlov_masteri','prep-1','mat-a','Mat A')")
        .execute(pool).await.unwrap();
    store
        .upsert_formula(&actor("prep-1"), formula("P", "A", &material))
        .await
        .unwrap();
    let restored = store.formula_sets("prep-1").await.unwrap();
    assert_eq!(restored["sets"][0]["set_id"], id);
    assert_eq!(restored["sets"][0]["name"], "Qurt saqlangan");

    // Exercise the database trigger directly, bypassing the service's owner lock.
    let insert = "INSERT INTO mini_preparation_formulas(owner_ref,product_code,material_id,material_name,name,lines)
                  VALUES ('prep-1','CONCURRENT','mat-a','Mat A',$1,'[]') RETURNING set_id";
    let (first, second) = tokio::join!(
        sqlx::query_scalar::<_, String>(insert)
            .bind("A")
            .fetch_one(&runtime),
        sqlx::query_scalar::<_, String>(insert)
            .bind("B")
            .fetch_one(&runtime),
    );
    let first = first.unwrap();
    assert_eq!(first, second.unwrap());
    assert_ne!(first, id);
    let all = store.formula_sets("prep-1").await.unwrap();
    assert_eq!(all["sets"].as_array().unwrap().len(), 2);
    assert!(
        all["sets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| !entry["name"].as_str().unwrap().is_empty())
    );
    // A supplied ID cannot move a formula into another owner's set.
    assert!(sqlx::query("INSERT INTO mini_preparation_formulas(owner_ref,product_code,material_id,material_name,name,lines,set_id) VALUES ('prep-2','P','mat-a','Mat A','Injected','[]',$1)")
        .bind(&id).execute(&runtime).await.is_err());
    runtime.close().await;
}
