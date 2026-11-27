use super::*;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tokio::time::timeout;

async fn changed(
    live: &FormulaLive,
    wake: &mut watch::Receiver<()>,
    base: Option<&FormulaCursor>,
) -> FormulaUpdate {
    timeout(Duration::from_secs(5), async {
        loop {
            wake.borrow_and_update();
            if let Some(update) = live.since("prep-1", base)
                && base != Some(&update.cursor)
            {
                return update;
            }
            wake.changed().await.unwrap();
        }
    })
    .await
    .expect("PostgreSQL formula change must reach the listener")
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn formula_library_commit_patches_both_lists_and_tracks_shared_recipe_orders() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let database = format!("mini_test_formula_live_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = url.parse::<PgConnectOptions>().unwrap().database(&database);
    let pool = PgPool::connect_with(options.clone()).await.unwrap();
    let checks = pool.clone();
    let result = tokio::spawn(async move { check_formula_live(checks, options).await }).await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    result.unwrap();
}

async fn check_formula_live(pool: PgPool, options: PgConnectOptions) {
    crate::db::postgres::apply_foundation_migration(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_system_users(id,role,name,phone) VALUES
          ('prep-1','tayyorlov_masteri','Master','901234567'),('prep-2','tayyorlov_masteri','Other','901234568');
        INSERT INTO mini_calculate_materials(id,lower_name,payload_json) VALUES
          ('mat-a','mat a','{"id":"mat-a","name":"Mat A","active":true}');
        INSERT INTO mini_preparation_material_responsibilities(principal_role,principal_ref,material_id,material_name)
          VALUES ('tayyorlov_masteri','prep-1','mat-a','Mat A'),('tayyorlov_masteri','prep-2','mat-a','Mat A');
        INSERT INTO mini_preparation_formula_variants(id,owner_ref,product_code,material_id,name) VALUES
          ('one-a','prep-1','SOURCE','mat-a','Recipe A'),('one-b','prep-1','OTHER','mat-a','Recipe B'),
          ('two-a','prep-2','SOURCE','mat-a','Other owner');
        INSERT INTO mini_preparation_formulas(owner_ref,product_code,material_id,material_name,name,lines,variant_id)
          VALUES ('prep-1','SOURCE','mat-a','Mat A','A','[]','one-a');
        INSERT INTO mini_orders(id,code,order_number,product_name)
          VALUES ('order-target','OT','OT','Target');
        INSERT INTO mini_production_maps(id,product_code,title,code,map_json)
          VALUES ('order-target','TARGET','Target','OT','{}');
        INSERT INTO mini_order_products(id,order_id,product_name,layers_json)
          VALUES ('item-target','order-target','Target','[{"material_id":"mat-a"}]');
        WITH recipe AS (
          INSERT INTO mini_preparation_formula_revisions(variant_id,content_key,formulas)
            VALUES ('one-a',public.mini_preparation_recipe_key('[]'),'[]') RETURNING id
        )
        INSERT INTO mini_preparation_order_formula_selections
          (owner_ref,order_id,variant_id,product_code,material_id,revision_id,display_name,is_pinned)
          SELECT 'prep-1','order-target','one-a','TARGET','mat-a',id,'Recipe A',true FROM recipe;
    "#).execute(&pool).await.unwrap();
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
    let live = FormulaLive::start(runtime.clone());
    let mut wake = live.subscribe();
    let base = changed(&live, &mut wake, None).await.cursor;
    let snapshot = store.formula_library_rows("prep-1", None).await.unwrap();
    assert_eq!(snapshot["sets"].as_array().unwrap().len(), 1);
    let source = snapshot["formulas"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "one-a")
        .unwrap();
    assert_eq!(source["order_codes"], json!(["OT"]));
    assert_eq!(snapshot["formulas"].as_array().unwrap().len(), 2);
    assert!(snapshot["sets"][0]["sort_at"].is_i64());

    let mut write = runtime.begin().await.unwrap();
    sqlx::query("UPDATE mini_preparation_formula_variants SET name='Renamed' WHERE id='one-a'")
        .execute(&mut *write)
        .await
        .unwrap();
    assert!(
        timeout(Duration::from_millis(50), wake.changed())
            .await
            .is_err()
    );
    write.commit().await.unwrap();
    let update = changed(&live, &mut wake, Some(&base)).await;
    assert_eq!(update.products.as_deref(), Some(&["SOURCE".to_owned()][..]));
    assert_eq!(
        live.since("prep-2", Some(&base)).unwrap().products,
        Some(vec![])
    );
    let patch = store
        .formula_library_rows("prep-1", update.products.as_deref())
        .await
        .unwrap();
    assert_eq!(patch["sets"][0]["formulas"][0]["variant_name"], "Renamed");
    assert_eq!(patch["formulas"].as_array().unwrap().len(), 1);
    assert_eq!(patch["formulas"][0]["name"], "Renamed");
    assert_eq!(patch["formulas"][0]["order_codes"], json!(["OT"]));

    // Updating only production metadata does not wake the formula feed. An
    // order-layer edit, on the other hand, affects the shared source recipe.
    sqlx::query("UPDATE mini_production_maps SET updated_at=now() WHERE id='order-target'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        timeout(Duration::from_millis(50), wake.changed())
            .await
            .is_err()
    );
    sqlx::query("UPDATE mini_order_products SET layers_json='[]' WHERE id='item-target'")
        .execute(&pool)
        .await
        .unwrap();
    let next = changed(&live, &mut wake, Some(&update.cursor)).await;
    assert_eq!(next.products, update.products);
    let patch = store
        .formula_library_rows("prep-1", next.products.as_deref())
        .await
        .unwrap();
    assert_eq!(patch["formulas"][0]["order_codes"], json!([]));

    sqlx::query("DELETE FROM mini_preparation_formulas WHERE variant_id='one-a'")
        .execute(&pool)
        .await
        .unwrap();
    let deletion = changed(&live, &mut wake, Some(&next.cursor)).await;
    let patch = store
        .formula_library_rows("prep-1", deletion.products.as_deref())
        .await
        .unwrap();
    assert_eq!(patch["sets"], json!([]));
    assert_eq!(patch["formulas"][0]["formula_count"], 0);
    runtime.close().await;
}
