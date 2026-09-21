use mini_rs_erp::core::{
    auth::models::{Principal, PrincipalRole},
    preparation::*,
};
use mini_rs_erp::db::{
    postgres::{apply_postgres_migrations_through_version, canonical_apparatus_service},
    postgres_preparation::PostgresPreparationStore,
};
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

fn actor() -> Principal {
    Principal {
        role: PrincipalRole::TayyorlovMasteri,
        ref_: "prep-1".into(),
        display_name: "Master".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    }
}
fn formula(material: &str, name: &str, lines: Vec<(&str, &str)>) -> FormulaUpsert {
    FormulaUpsert {
        product_code: "P".into(),
        material_id: material.into(),
        name: name.into(),
        lines: lines
            .into_iter()
            .map(|(code, percent)| FormulaLine {
                item_code: code.into(),
                percent: percent.into(),
            })
            .collect(),
    }
}
fn ids(data: &Value) -> Vec<String> {
    let mut ids = data["orders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["order_id"].as_str().unwrap().into())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn formula_orders_scope_edit_delete_and_shared_product() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .try_init();
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let db = format!("mini_test_formula_orders_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = url.parse::<PgConnectOptions>().unwrap().database(&db);
    let pool = PgPool::connect_with(options.clone()).await.unwrap();
    let checks = pool.clone();
    let result = tokio::spawn(async move { run_checks(&checks, options).await }).await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db} WITH (FORCE)"))
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
    sqlx::raw_sql("INSERT INTO mini_system_users(id,role,name,phone) VALUES
        ('prep-1','tayyorlov_masteri','Master','901234567'),('prep-2','tayyorlov_masteri','Other','901234568');
        INSERT INTO mini_warehouses(id,name) VALUES ('own','Own');
        INSERT INTO mini_warehouse_assignments(assignment_kind,warehouse,warehouse_name,principal_role,principal_ref)
        VALUES ('warehouse','Own','Own','tayyorlov_masteri','prep-1');
        INSERT INTO mini_calculate_materials(id,lower_name,payload_json) VALUES
          ('mat-a','mat a','{\"id\":\"mat-a\",\"name\":\"Mat A\",\"active\":true}'),
          ('mat-b','mat b','{\"id\":\"mat-b\",\"name\":\"Mat B\",\"active\":true}');
        INSERT INTO mini_preparation_material_responsibilities(principal_role,principal_ref,material_id,material_name)
        VALUES ('tayyorlov_masteri','prep-1','mat-a','Mat A'),('tayyorlov_masteri','prep-1','mat-b','Mat B'),
               ('tayyorlov_masteri','prep-2','mat-a','Mat A');
        INSERT INTO mini_orders(id,code,order_number,product_name) VALUES
          ('o1','O1','O1','Product'),('o2','O2','O2','Product'),('stale','STALE','STALE','Product'),('empty','EMPTY','EMPTY','Empty');
        INSERT INTO mini_production_maps(id,product_code,title,code,map_json) VALUES
          ('o1','P','Product','O1','{}'),('o2','P','Product','O2','{}'),
          ('stale','P','Product','STALE','{}'),('empty','NO-FORMULA','Empty','EMPTY','{}'),
          ('legacy','P','Legacy order','LEGACY','{}'),('unrelated','P','No matching scope','UNRELATED','{}');
        INSERT INTO mini_order_products(id,order_id,product_name,layers_json) VALUES
          ('p1','o1','Product','[{\"material_id\":\"mat-a\"},{\"material_id\":\"mat-b\"}]'),
          ('p2','o2','Product','[{\"material_id\":\"mat-a\"}]'),
          ('p3','stale','Product','[{\"material_id\":\"outside\",\"material\":\"Outside\"}]'),
          ('p4','empty','Empty','[{\"material_id\":\"mat-a\"}]');
        INSERT INTO mini_quick_order_templates(id,owner_key,code,name,product_name,quick_key,payload_json) VALUES
          ('t1','test','LEGACY','Legacy','Product','legacy','{\"source_map_id\":\"legacy\",\"first_layer_material\":\"Mat A\"}'),
          ('t2','test','STALE','Stale','Product','stale','{\"source_map_id\":\"stale\",\"layers\":[{\"material_id\":\"mat-a\"}]}'),
          ('t3','test','UNRELATED','Unrelated','Product','unrelated','{\"source_map_id\":\"different-order\",\"layers\":[{\"material_id\":\"mat-a\"}]}');")
        .execute(pool).await.unwrap();
    let runtime = PgPoolOptions::new()
        .after_connect(|conn, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp").execute(conn).await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .unwrap();
    let store = PostgresPreparationStore::new(runtime.clone());
    assert!(ids(&store.formula_orders("prep-1").await.unwrap()).is_empty());
    let mut codes = Vec::new();
    for name in ["Kley", "Un"] {
        let created = store
            .create_material(
                &actor(),
                MaterialCreate {
                    request_id: format!("material-{name}"),
                    name: name.into(),
                    warehouse: "Own".into(),
                },
            )
            .await
            .unwrap();
        codes.push(created["item_code"].as_str().unwrap().to_string());
    }
    store
        .upsert_formula(&actor(), formula("mat-a", "A", vec![(&codes[0], "100")]))
        .await
        .unwrap();
    store
        .upsert_formula(&actor(), formula("mat-b", "B", vec![(&codes[1], "100")]))
        .await
        .unwrap();
    sqlx::query("INSERT INTO mini_preparation_formulas(owner_ref,product_code,material_id,material_name,name,lines)
      VALUES ('prep-2','P','mat-a','Mat A','Foreign',$1)")
        .bind(json!([{"item_code":"foreign","name":"Private","percent":"100"}])).execute(pool).await.unwrap();
    let listed = store.formula_orders("prep-1").await.unwrap();
    assert_eq!(ids(&listed), ["legacy", "o1", "o2"]);
    let o1 = listed["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["order_id"] == "o1")
        .unwrap();
    assert_eq!(o1["formulas"].as_array().unwrap().len(), 2);
    assert!(!listed.to_string().contains("Private"));
    assert!(ids(&store.formula_orders("unknown").await.unwrap()).is_empty());
    store
        .upsert_formula(
            &actor(),
            formula("mat-a", "A", vec![(&codes[0], "60"), (&codes[1], "40")]),
        )
        .await
        .unwrap();
    let listed = store.formula_orders("prep-1").await.unwrap();
    for order in listed["orders"].as_array().unwrap() {
        let saved = order["formulas"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == "A")
            .unwrap();
        assert_eq!(saved["lines"].as_array().unwrap().len(), 2);
        assert_eq!(saved["lines"][0]["percent"], "60.000000");
    }
    assert!(
        store
            .delete_formula("prep-1", "P", "Foreign", "mat-a")
            .await
            .is_err()
    );
    sqlx::query("DELETE FROM mini_preparation_material_responsibilities WHERE principal_ref='prep-1' AND material_id='mat-b'")
        .execute(pool).await.unwrap();
    assert!(matches!(
        store.delete_formula("prep-1", "P", "B", "mat-b").await,
        Err(PreparationError::Forbidden)
    ));
    let listed = store.formula_orders("prep-1").await.unwrap();
    assert_eq!(ids(&listed), ["legacy", "o1", "o2", "saved:P"]);
    let saved = listed["orders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["order_id"] == "saved:P")
        .unwrap();
    assert_eq!(saved["has_order"], false);
    assert_eq!(saved["formulas"].as_array().unwrap().len(), 1);
    assert_eq!(saved["formulas"][0]["name"], "B");
    // Revoked scope permits management of an existing owned formula only.
    store
        .update_saved_formula(&actor(), formula("mat-b", "B", vec![(&codes[1], "100")]))
        .await
        .unwrap();
    assert!(matches!(
        store
            .upsert_formula(&actor(), formula("mat-b", "C", vec![(&codes[1], "100")]))
            .await,
        Err(PreparationError::Forbidden)
    ));
    store
        .delete_saved_formula("prep-1", "P", "B", "mat-b")
        .await
        .unwrap();
    store
        .delete_formula("prep-1", "P", "A", "mat-a")
        .await
        .unwrap();
    assert!(ids(&store.formula_orders("prep-1").await.unwrap()).is_empty());
    assert_eq!(
        ids(&store.formula_orders("prep-2").await.unwrap()),
        ["legacy", "o1", "o2"]
    );
    check_saved_formulas(pool, &store, &codes).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM mini_preparation_operations WHERE kind='consumption'"
        )
        .fetch_one(pool)
        .await
        .unwrap(),
        0
    );
    runtime.close().await;
}

async fn check_saved_formulas(pool: &PgPool, store: &PostgresPreparationStore, codes: &[String]) {
    // Same product/name across legacy and scoped formulas must stay distinct.
    sqlx::query("INSERT INTO mini_preparation_formulas(owner_ref,product_code,material_id,material_name,name,lines)
        VALUES ('prep-1','ORPHAN','','','A',$1),('prep-1','ORPHAN','','','Asosiy',$1),
               ('prep-1','ORPHAN','mat-a','Mat A','A',$1)")
        .bind(json!([{"item_code":codes[0],"name":"Kley","percent":"100"}]))
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO mini_quick_order_templates(id,owner_key,code,name,item_code,product_name,quick_key,payload_json)
        VALUES ('orphan-template','test','ORPHAN','Template','ORPHAN','Nilo maffin 180 gr','orphan','{}'),
               ('orphan-duplicate','test','ORPHAN-2','Template','ORPHAN','Nilo maffin 180 gr','orphan2','{}')")
        .execute(pool).await.unwrap();
    let listed = store.formula_orders("prep-1").await.unwrap();
    assert_eq!(ids(&listed), ["saved:ORPHAN"]);
    let product = &listed["orders"][0];
    assert_eq!(product["title"], "Nilo maffin 180 gr");
    assert_eq!(product["has_order"], false);
    assert_eq!(product["order_code"], "");
    assert_eq!(product["formulas"].as_array().unwrap().len(), 3);
    assert_eq!(
        store
            .list_saved_formulas("prep-1", "ORPHAN", "")
            .await
            .unwrap()["formulas"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        store
            .list_saved_formulas("prep-2", "ORPHAN", "")
            .await
            .unwrap()["formulas"],
        json!([])
    );
    let mut input = formula("", "A", vec![(&codes[0], "70"), (&codes[1], "30")]);
    input.product_code = "ORPHAN".into();
    let updated = store
        .update_saved_formula(&actor(), input.clone())
        .await
        .unwrap();
    assert_eq!(updated["material_id"], "");
    assert_eq!(updated["lines"][0]["percent"], "70.000000");
    assert_eq!(
        store
            .list_saved_formulas("prep-1", "ORPHAN", "mat-a")
            .await
            .unwrap()["formulas"][0]["lines"][0]["percent"],
        "100"
    );
    let mut foreign = actor();
    foreign.ref_ = "prep-2".into();
    assert!(
        store
            .update_saved_formula(&foreign, input.clone())
            .await
            .is_err()
    );
    assert!(
        store
            .delete_saved_formula("prep-2", "ORPHAN", "A", "")
            .await
            .is_err()
    );
    for (name, material, percent, code) in [
        ("New", "", "100", codes[0].as_str()),
        ("A", "outside", "100", codes[0].as_str()),
        ("A", "", "99", codes[0].as_str()),
        ("A", "", "100", "foreign-material"),
    ] {
        let mut invalid = formula(material, name, vec![(code, percent)]);
        invalid.product_code = "ORPHAN".into();
        assert!(store.update_saved_formula(&actor(), invalid).await.is_err());
    }
    assert!(matches!(
        store.delete_owned_material("prep-1", &codes[0]).await,
        Err(PreparationError::MaterialInUse)
    ));
    store
        .delete_saved_formula("prep-1", "ORPHAN", "A", "")
        .await
        .unwrap();
    assert!(store.update_saved_formula(&actor(), input).await.is_err());
    assert_eq!(
        store
            .list_saved_formulas("prep-1", "ORPHAN", "")
            .await
            .unwrap()["formulas"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store.formula_orders("prep-1").await.unwrap()["orders"][0]["formulas"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    store
        .delete_saved_formula("prep-1", "ORPHAN", "Asosiy", "")
        .await
        .unwrap();
    // The other scope still protects this material.
    assert!(matches!(
        store.delete_owned_material("prep-1", &codes[0]).await,
        Err(PreparationError::MaterialInUse)
    ));
    store
        .delete_saved_formula("prep-1", "ORPHAN", "A", "mat-a")
        .await
        .unwrap();
    assert!(ids(&store.formula_orders("prep-1").await.unwrap()).is_empty());
    store
        .delete_owned_material("prep-1", &codes[0])
        .await
        .unwrap();
    assert_eq!(
        ids(&store.formula_orders("prep-2").await.unwrap()),
        ["legacy", "o1", "o2"]
    );
}
