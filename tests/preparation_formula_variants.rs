use mini_rs_erp::{
    core::{
        auth::models::{Principal, PrincipalRole},
        preparation::*,
    },
    db::{
        postgres::{apply_postgres_migrations_through_version, canonical_apparatus_service},
        postgres_preparation::PostgresPreparationStore,
    },
};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

fn create(name: &str, request: &str) -> FormulaVariantCreate {
    FormulaVariantCreate {
        product_code: "P".into(),
        material_id: "mat-a".into(),
        name: name.into(),
        request_id: request.into(),
    }
}
fn allocation(values: &[(&str, &str)]) -> FormulaSharesUpdate {
    FormulaSharesUpdate {
        product_code: "P".into(),
        material_id: "mat-a".into(),
        shares: values
            .iter()
            .map(|(name, p)| FormulaShare {
                name: (*name).into(),
                percent: (*p).into(),
            })
            .collect(),
    }
}

#[test]
fn variant_names_are_required_and_normalized() {
    assert_eq!(
        create("  Yangi   qatlam  ", "one")
            .normalized_name()
            .unwrap(),
        "Yangi qatlam"
    );
    for name in ["", "  ", &"x".repeat(161)] {
        assert!(create(name, "one").normalized_name().is_err());
    }
    let mut invalid = create("Name", "");
    assert!(invalid.normalized_name().is_err());
    invalid.request_id = "one".into();
    invalid.material_id = "".into();
    assert!(invalid.normalized_name().is_err());
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn named_variants_persist_empty_and_isolate_screws_and_shares() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let database = format!("mini_test_formula_variants_{:016x}", rand::random::<u64>());
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
          ('prep-1','tayyorlov_masteri','Master','901234567'),('prep-2','tayyorlov_masteri','Other','901234568');
        INSERT INTO mini_warehouses(id,name) VALUES ('own','Own');
        INSERT INTO mini_warehouse_assignments(assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
          VALUES ('warehouse','Own','Own','tayyorlov_masteri','prep-1');
        INSERT INTO mini_calculate_materials(id,lower_name,payload_json) VALUES
          ('mat-a','mat a','{"id":"mat-a","name":"Mat A","active":true}');
        INSERT INTO mini_preparation_material_responsibilities(principal_role,principal_ref,material_id,material_name)
          VALUES ('tayyorlov_masteri','prep-1','mat-a','Mat A'),('tayyorlov_masteri','prep-2','mat-a','Mat A');
        INSERT INTO mini_orders(id,code,order_number,product_name)
          VALUES ('order-1','O1','O1','Product');
        INSERT INTO mini_production_maps(id,product_code,title,code,map_json)
          VALUES ('order-1','P','Product','O1','{}');
        INSERT INTO mini_order_products(id,order_id,product_name,layers_json)
          VALUES ('item-1','order-1','Product','[{"material_id":"mat-a"}]');
        INSERT INTO mini_preparation_formulas(owner_ref,product_code,material_id,material_name,name,lines) VALUES
          ('prep-1','P','mat-a','Mat A','A','[]'),('prep-1','P','mat-a','Mat A','B','[]'),
          ('prep-1','P','mat-a','Mat A','C','[]');
    "#).execute(pool).await.unwrap();
    for migration in [
        include_str!("../migrations/postgres/0158_preparation_formula_sets.sql"),
        include_str!("../migrations/postgres/0159_preparation_screw_shares.sql"),
    ] {
        sqlx::raw_sql(migration).execute(pool).await.unwrap();
    }
    sqlx::query("UPDATE mini_preparation_formulas SET share_percent=CASE name WHEN 'A' THEN 30 WHEN 'B' THEN 40 ELSE 30 END")
        .execute(pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/postgres/0160_preparation_formula_variants.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("../migrations/postgres/0175_preparation_formula_codes.sql"))
        .execute(pool).await.unwrap();
    sqlx::raw_sql(include_str!("../migrations/postgres/0176_preparation_recipe_codes.sql"))
        .execute(pool).await.unwrap();
    let runtime = PgPoolOptions::new()
        .after_connect(|c, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp").execute(c).await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .unwrap();
    let store = PostgresPreparationStore::new(runtime.clone());
    let old = store
        .formula_variants("prep-1", "P", "mat-a", false)
        .await
        .unwrap();
    assert_eq!(old["variants"].as_array().unwrap().len(), 1);
    assert_eq!(old["variants"][0]["name"], "Asosiy");
    assert_eq!(old["variants"][0]["formula_count"], 3);
    assert_eq!(old["variants"][0]["shares_complete"], true);
    let old_id = old["variants"][0]["id"].as_str().unwrap().to_owned();
    let old_code = old["variants"][0]["formula_code"].as_str().unwrap().to_owned();
    assert!(old_code.starts_with("9504") && old_code.len() == 12);
    let initial_catalog = store.formula_catalog("prep-1").await.unwrap();
    assert_eq!(initial_catalog["formulas"].as_array().unwrap().len(), 1);
    assert_eq!(initial_catalog["formulas"][0]["formula_code"], old_code);
    assert_eq!(initial_catalog["formulas"][0]["formula_count"], 3);
    assert_eq!(initial_catalog["formulas"][0]["order_codes"], serde_json::json!(["O1"]));
    assert_eq!(initial_catalog["formulas"][0]["can_create"], true);
    assert!(store.formula_catalog("prep-2").await.unwrap()["formulas"].as_array().unwrap().is_empty());
    let old_formulas = store.list_formulas("prep-1", "P", "mat-a").await.unwrap();
    let created = store
        .create_formula_variant("prep-1", create("  Yangi   qatlam ", "one"))
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap().to_owned();
    let recipe_code = created["formula_code"].as_str().unwrap().to_owned();
    assert_ne!(recipe_code, old_code);
    assert!(recipe_code.starts_with("9504") && recipe_code.len() == 12);
    assert!(sqlx::query("UPDATE mini_preparation_formula_variants SET formula_code='950499999999' WHERE id=$1")
        .bind(&id).execute(pool).await.is_err());
    assert_ne!(id, old_id);
    assert_eq!(created["name"], "Yangi qatlam");
    let retry = store
            .create_formula_variant("prep-1", create("Yangi qatlam", "one"))
            .await
            .unwrap();
    assert_eq!(retry["id"], id);
    assert_eq!(retry["formula_code"], recipe_code);
    assert!(
        store
            .create_formula_variant("prep-1", create("Other", "one"))
            .await
            .is_err()
    );
    assert!(
        store
            .create_formula_variant("prep-1", create("yangi qatlam", "two"))
            .await
            .is_err()
    );
    assert!(
        store
            .list_formulas_variant("prep-1", "P", "mat-a", &id)
            .await
            .unwrap()["formulas"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let variants = store
        .formula_variants("prep-1", "P", "mat-a", false)
        .await
        .unwrap();
    assert_eq!(variants["variants"].as_array().unwrap().len(), 2);
    assert_eq!(variants["variants"][1]["formula_count"], 0);
    let empty_catalog = store.formula_catalog("prep-1").await.unwrap();
    let empty = empty_catalog["formulas"].as_array().unwrap().iter().find(|f| f["id"] == id).unwrap();
    assert_eq!(empty["formula_code"], recipe_code);
    assert_eq!(empty["formula_count"], 0);
    let foreign_id = store
        .create_formula_variant("prep-2", create("Yangi qatlam", "one"))
        .await
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(foreign_id, id);
    let foreign_catalog = store.formula_catalog("prep-2").await.unwrap();
    assert_eq!(foreign_catalog["formulas"].as_array().unwrap().len(), 1);
    assert_ne!(foreign_catalog["formulas"][0]["formula_code"], recipe_code);
    let actor = Principal {
        role: PrincipalRole::TayyorlovMasteri,
        ref_: "prep-1".into(),
        display_name: "Master".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let item = store
        .create_material(
            &actor,
            MaterialCreate {
                request_id: "variant-series-1".into(),
                name: "0209".into(),
                warehouse: "Own".into(),
            },
        )
        .await
        .unwrap()["item_code"]
        .as_str()
        .unwrap()
        .to_owned();
    let formula = |name: &str| FormulaUpsert {
        product_code: "P".into(),
        material_id: "mat-a".into(),
        name: name.into(),
        lines: vec![FormulaLine {
            item_code: item.clone(),
            percent: "100".into(),
        }],
    };
    assert_eq!(
        store
            .upsert_formula_variant(&actor, formula("A"), &id)
            .await
            .unwrap()["share_percent"],
        "100.000000"
    );
    store
        .upsert_formula_variant(&actor, formula("B"), &id)
        .await
        .unwrap();
    store
        .upsert_formula_variant(&actor, formula("C"), &id)
        .await
        .unwrap();
    let updated = store
        .update_formula_variant_shares(
            "prep-1",
            allocation(&[("A", "20"), ("B", "60"), ("C", "20")]),
            false,
            &id,
        )
        .await
        .unwrap();
    assert_eq!(updated["formulas"][0]["share_percent"], "20.000000");
    let catalog = store.formula_catalog("prep-1").await.unwrap();
    let recipe = catalog["formulas"].as_array().unwrap().iter().find(|f| f["id"] == id).unwrap();
    assert_eq!(recipe["formula_code"], recipe_code);
    assert_eq!(recipe["formula_count"], 3);
    assert_eq!(recipe["shares_complete"], true);
    assert_eq!(
        store.list_formulas("prep-1", "P", "mat-a").await.unwrap(),
        old_formulas
    );
    assert_eq!(
        store
            .update_saved_formula_variant(&actor, formula("A"), &id)
            .await
            .unwrap()["share_percent"],
        "20.000000"
    );
    assert!(
        store
            .upsert_formula_variant(&actor, formula("A"), &foreign_id)
            .await
            .is_err()
    );
    assert!(
        store
            .delete_formula_variant("prep-1", "P", "A", "mat-a", &foreign_id)
            .await
            .is_err()
    );
    assert!(
        store
            .update_formula_variant_shares("prep-1", allocation(&[("A", "100")]), true, &foreign_id)
            .await
            .is_err()
    );
    store
        .delete_saved_formula_variant("prep-1", "P", "B", "mat-a", &id)
        .await
        .unwrap();
    assert_eq!(
        store
            .list_saved_formulas_variant("prep-1", "P", "mat-a", &id)
            .await
            .unwrap()["formulas"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        store.list_formulas("prep-1", "P", "mat-a").await.unwrap(),
        old_formulas
    );
    let sets = store.formula_sets("prep-1").await.unwrap();
    assert_eq!(sets["sets"][0]["formulas"].as_array().unwrap().len(), 5);
    assert!(
        sets["sets"][0]["formulas"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["variant_id"] == id && f["variant_name"] == "Yangi qatlam")
    );
    let (first, second) = tokio::join!(
        store.create_formula_variant("prep-1", create("Concurrent", "parallel")),
        store.create_formula_variant("prep-1", create("Concurrent", "parallel"))
    );
    assert_eq!(first.unwrap()["id"], second.unwrap()["id"]);
    sqlx::query("DELETE FROM mini_preparation_material_responsibilities WHERE principal_ref='prep-1'")
        .execute(pool).await.unwrap();
    let catalog = store.formula_catalog("prep-1").await.unwrap();
    let recipe = catalog["formulas"].as_array().unwrap().iter().find(|f| f["id"] == id).unwrap();
    assert_eq!(recipe["formula_code"], recipe_code);
    assert_eq!(recipe["material_name"], "Mat A");
    assert_eq!(recipe["order_codes"], serde_json::json!([]));
    assert_eq!(recipe["can_create"], false);
    runtime.close().await;
}
