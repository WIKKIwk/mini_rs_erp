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
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

fn shares(values: &[(&str, &str)]) -> FormulaSharesUpdate {
    FormulaSharesUpdate {
        product_code: "P".into(),
        material_id: "mat-a".into(),
        shares: values
            .iter()
            .map(|(name, percent)| FormulaShare {
                name: (*name).into(),
                percent: (*percent).into(),
            })
            .collect(),
    }
}

#[test]
fn screw_shares_require_exact_hundred_and_unique_names() {
    assert!(
        shares(&[("A", "30"), ("B", "40"), ("C", "30")])
            .normalized()
            .is_ok()
    );
    assert!(
        shares(&[("A", "33.333333"), ("B", "33.333333"), ("C", "33.333334")])
            .normalized()
            .is_ok()
    );
    for invalid in [
        vec![],
        vec![("A", "101")],
        vec![("A", "50"), ("A", "50")],
        vec![("A", "0"), ("B", "100")],
        vec![("A", "-10"), ("B", "110")],
        vec![("A", "99.999999")],
        vec![("A", "NaN")],
        vec![("", "100")],
        vec![("A", "1e2")],
        vec![("A", "100.0000001")],
    ] {
        assert!(shares(&invalid).normalized().is_err(), "{invalid:?}");
    }
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn screw_shares_migrate_persist_scope_and_update_atomically() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let database = format!("mini_test_screw_shares_{:016x}", rand::random::<u64>());
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
        INSERT INTO mini_calculate_materials(id,lower_name,payload_json) VALUES
          ('mat-a','mat a','{"id":"mat-a","name":"Mat A","active":true}'),
          ('mat-b','mat b','{"id":"mat-b","name":"Mat B","active":true}');
        INSERT INTO mini_preparation_material_responsibilities(principal_role,principal_ref,material_id,material_name)
          VALUES ('tayyorlov_masteri','prep-1','mat-a','Mat A');
        INSERT INTO mini_preparation_formulas(owner_ref,product_code,material_id,material_name,name,lines) VALUES
          ('prep-1','legacy-single','mat-a','Mat A','A','[]'),
          ('prep-1','legacy-multi','mat-a','Mat A','A','[]'),
          ('prep-1','legacy-multi','mat-a','Mat A','B','[]'),
          ('prep-1','P','mat-b','Mat B','A','[]'),
          ('prep-2','P','mat-a','Mat A','Foreign','[]');
    "#).execute(pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/postgres/0158_preparation_formula_sets.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/postgres/0159_preparation_screw_shares.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("../migrations/postgres/0160_preparation_formula_variants.sql"))
        .execute(pool).await.unwrap();
    sqlx::raw_sql(include_str!("../migrations/postgres/0175_preparation_formula_codes.sql"))
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
    assert_eq!(
        store
            .list_formulas("prep-1", "legacy-single", "mat-a")
            .await
            .unwrap()["formulas"][0]["share_percent"],
        "100.000000"
    );
    for f in store
        .list_formulas("prep-1", "legacy-multi", "mat-a")
        .await
        .unwrap()["formulas"]
        .as_array()
        .unwrap()
    {
        assert!(f["share_percent"].is_null());
    }
    let actor = Principal {
        role: PrincipalRole::TayyorlovMasteri,
        ref_: "prep-1".into(),
        display_name: "Master".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let material = store
        .create_material(
            &actor,
            MaterialCreate {
                request_id: "screw-series".into(),
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
            item_code: material.clone(),
            percent: "100".into(),
        }],
    };
    assert_eq!(
        store.upsert_formula(&actor, formula("A")).await.unwrap()["share_percent"],
        "100.000000"
    );
    assert!(store.upsert_formula(&actor, formula("B")).await.unwrap()["share_percent"].is_null());
    assert!(store.upsert_formula(&actor, formula("C")).await.unwrap()["share_percent"].is_null());
    for invalid in [
        shares(&[("A", "50"), ("B", "50")]),
        shares(&[("A", "30"), ("B", "30"), ("C", "30")]),
        shares(&[("A", "30"), ("B", "40"), ("missing", "30")]),
    ] {
        assert!(
            store
                .update_formula_shares("prep-1", invalid, false)
                .await
                .is_err()
        );
    }
    assert!(
        store
            .update_formula_shares(
                "prep-2",
                shares(&[("A", "30"), ("B", "40"), ("C", "30")]),
                true
            )
            .await
            .is_err()
    );
    let updated = store
        .update_formula_shares(
            "prep-1",
            shares(&[("C", "30"), ("A", "30"), ("B", "40")]),
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        updated["formulas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["share_percent"].clone())
            .collect::<Vec<_>>(),
        vec![json!("30.000000"), json!("40.000000"), json!("30.000000")]
    );
    assert_eq!(
        store.upsert_formula(&actor, formula("A")).await.unwrap()["share_percent"],
        "30.000000"
    );
    assert_eq!(
        store
            .update_saved_formula(&actor, formula("A"))
            .await
            .unwrap()["share_percent"],
        "30.000000"
    );
    assert_eq!(
        store
            .list_saved_formulas("prep-1", "P", "mat-b")
            .await
            .unwrap()["formulas"][0]["share_percent"],
        "100.000000"
    );
    assert_eq!(
        store
            .list_saved_formulas("prep-2", "P", "mat-a")
            .await
            .unwrap()["formulas"][0]["share_percent"],
        "100.000000"
    );
    let sets = store.formula_sets("prep-1").await.unwrap();
    let p = sets["sets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["product_code"] == "P")
        .unwrap();
    assert!(
        p["formulas"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["material_id"] == "mat-a"
                && f["name"] == "B"
                && f["share_percent"] == "40.000000")
    );
    store
        .delete_formula("prep-1", "P", "C", "mat-a")
        .await
        .unwrap();
    assert!(
        store
            .update_formula_shares(
                "prep-1",
                shares(&[("A", "30"), ("B", "40"), ("C", "30")]),
                false
            )
            .await
            .is_err()
    );
    let (one, two) = tokio::join!(
        store.update_formula_shares("prep-1", shares(&[("A", "20"), ("B", "80")]), false),
        store.update_formula_shares("prep-1", shares(&[("A", "60"), ("B", "40")]), true),
    );
    one.unwrap();
    two.unwrap();
    let final_shares: Vec<Value> =
        store.list_formulas("prep-1", "P", "mat-a").await.unwrap()["formulas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["share_percent"].clone())
            .collect();
    assert!(
        final_shares == vec![json!("20.000000"), json!("80.000000")]
            || final_shares == vec![json!("60.000000"), json!("40.000000")]
    );
    store
        .delete_saved_formula("prep-1", "P", "B", "mat-a")
        .await
        .unwrap();
    store
        .update_formula_shares("prep-1", shares(&[("A", "100")]), false)
        .await
        .unwrap();
    runtime.close().await;
}
