use super::*;
use crate::core::admin::ports::AdminReadPort;
use crate::core::formula::LayerInput;
use crate::core::order_layout::OrderProduct;
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    canonical_apparatus_service, postgres_test_database_options,
};
use crate::db::postgres_admin_catalog::PostgresAdminCatalogStore;

mod qolip_tests;

fn template() -> CalculateOrderTemplate {
    crate::core::calculate_orders::hydrate_template_dimensions(CalculateOrderTemplate {
        customer_ref: "combo-customer".into(),
        customer: "Combo customer".into(),
        products: ["A", "B"]
            .into_iter()
            .map(|code| OrderProduct {
                id: format!("row-{code}"),
                item_code: code.into(),
                product: code.into(),
                frame_product_size_mm: 100.0,
                frame_count: 1,
            })
            .collect(),
        layers: vec![LayerInput::new("pet", "12")],
        edge_allowance_mm: 15.0,
        status: "Rulon".into(),
        kg: 300.0,
        waste_percent: 5.0,
        ..Default::default()
    })
}

#[tokio::test]
#[ignore = "requires an explicit disposable MINI_ERP_TEST_ADMIN_DATABASE_URL"]
async fn combo_catalog_persists_revisions_and_runtime_guards() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").unwrap();
    let server = PgPool::connect(&url).await.unwrap();
    let database = format!("combo_test_{:032x}", rand::random::<u128>());
    sqlx::query(&format!("CREATE DATABASE \"{database}\""))
        .execute(&server)
        .await
        .unwrap();
    let admin = PgPool::connect_with(postgres_test_database_options(&url, &database))
        .await
        .unwrap();
    apply_postgres_migrations_through_version(&admin, "0121")
        .await
        .unwrap();
    canonical_apparatus_service(admin.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    apply_foundation_migration(&admin).await.unwrap();
    sqlx::raw_sql("INSERT INTO mini_customers(ref,name) VALUES ('combo-customer','Combo customer');
        INSERT INTO mini_items(code,name,uom,item_group) VALUES
        ('A','A','Kg','Tayyor mahsulot'),('B','B','Kg','Tayyor mahsulot');
        INSERT INTO mini_customer_items(customer_ref,item_code) VALUES ('combo-customer','A'),('combo-customer','B');")
        .execute(&admin).await.unwrap();
    let runtime = PgPool::connect_with(
        postgres_test_database_options(&url, &database).username("mini_rs_erp"),
    )
    .await
    .unwrap();
    let store = PostgresComboStore::new(runtime.clone());
    let initial = template();
    let first = store.save("Morning combo", None, &initial).await.unwrap();
    assert_eq!(first.revision, 1);
    let entry = store.get(&first.item_code).await.unwrap().unwrap();
    store.validate(&entry.template).await.unwrap();
    let mut stripped = entry.template.clone();
    stripped.combo = None;
    assert!(matches!(
        store.validate(&stripped).await,
        Err(ComboError::InvalidInput(_))
    ));
    let mut forged = entry.template.clone();
    forged.combo.as_mut().unwrap().name = "Different owner".into();
    assert_eq!(
        store.validate(&forged).await,
        Err(ComboError::ReferenceMismatch)
    );
    let detail = PostgresAdminCatalogStore::new(runtime.clone())
        .item_detail(&first.item_code)
        .await
        .unwrap();
    assert_eq!(detail.name, "Morning combo · v1");
    assert_eq!(detail.item_group, "Kombo");
    assert!(detail.is_finished_goods);
    assert_eq!(detail.customers[0].ref_, "combo-customer");
    qolip_tests::assert_combo_mold_attachment(&admin, &runtime, &first).await;
    let mut quantity = initial.clone();
    quantity.kg = 600.0;
    quantity.waste_percent = 7.0;
    let (retry_a, retry_b) = tokio::join!(
        store.save("Morning combo", None, &quantity),
        store.save("Morning combo", None, &quantity)
    );
    assert_eq!(retry_a.unwrap(), first);
    assert_eq!(retry_b.unwrap(), first);
    let mut changed = entry.template.clone();
    changed.products[0].frame_product_size_mm = 110.0;
    assert!(store.validate(&changed).await.is_err());
    let second = store
        .save("Morning combo", Some(&first.id), &changed)
        .await
        .unwrap();
    assert_eq!(second.revision, 2);
    assert_ne!(first.item_code, second.item_code);
    assert_eq!(store.get(&first.item_code).await.unwrap().unwrap(), entry);
    assert!(
        matches!(store.save("Morning combo", None, &changed).await, Ok(reference) if reference == second)
    );
    let mut conflict = changed.clone();
    conflict.combo = None;
    conflict.products[1].frame_count = 3;
    assert_eq!(
        store.save("Morning combo", None, &conflict).await,
        Err(ComboError::NameConflict)
    );
    let mut nested = initial.clone();
    nested.products[0].item_code = second.item_code.clone();
    assert!(matches!(
        store.save("Nested", None, &nested).await,
        Err(ComboError::InvalidInput(_))
    ));
    for (sql, reason) in [
        (
            "UPDATE mini_items SET name='Wrong' WHERE code=$1",
            "combo_catalog_immutable",
        ),
        (
            "UPDATE mini_items SET item_group='Tayyor mahsulot' WHERE code=$1",
            "combo_catalog_immutable",
        ),
        (
            "DELETE FROM mini_items WHERE code=$1",
            "combo_catalog_immutable",
        ),
        (
            "DELETE FROM mini_customer_items WHERE item_code=$1",
            "combo_customer_immutable",
        ),
        (
            "INSERT INTO mini_finished_goods_stock(id,warehouse,item_code,qty) VALUES ('combo-stock','Finished',$1,1)",
            "combo_is_not_finished_stock",
        ),
        (
            "INSERT INTO mini_raw_material_stock(id,warehouse,item_code,barcode,qty) VALUES ('combo-raw','Raw',$1,'COMBO-BARCODE',1)",
            "combo_is_not_finished_stock",
        ),
    ] {
        let error = sqlx::query(sql)
            .bind(&first.item_code)
            .execute(&runtime)
            .await
            .unwrap_err();
        assert!(error.to_string().contains(reason), "{sql}: {error}");
    }
    for sql in [
        "UPDATE mini_items SET code='RENAMED-A' WHERE code='A'",
        "DELETE FROM mini_items WHERE code='A'",
    ] {
        let error = sqlx::query(sql).execute(&runtime).await.unwrap_err();
        assert!(
            error.to_string().contains("combo_component_in_use"),
            "{sql}: {error}"
        );
    }
    // Child display names are metadata; unused ordinary identities remain editable.
    sqlx::query("UPDATE mini_items SET name='A display name' WHERE code='A'")
        .execute(&runtime)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO mini_items(code,name,item_group) VALUES ('UNUSED','Unused','All Item Groups')",
    )
    .execute(&runtime)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE mini_items SET code='UNUSED-RENAMED',name='Renamed unused' WHERE code='UNUSED'",
    )
    .execute(&runtime)
    .await
    .unwrap();
    let error =
        sqlx::query("UPDATE mini_combo_revisions SET template_json='{}' WHERE item_code=$1")
            .bind(&first.item_code)
            .execute(&admin)
            .await
            .unwrap_err();
    assert!(
        error.to_string().contains("combo_revision_immutable"),
        "{error}"
    );
    let error = sqlx::query(
        "INSERT INTO mini_items(code,name,item_group) VALUES ('BAD-COMBO','Bad','Kombo')",
    )
    .execute(&runtime)
    .await
    .unwrap_err();
    assert!(
        error.to_string().contains("combo_catalog_reserved"),
        "{error}"
    );
    sqlx::query("INSERT INTO mini_finished_goods_stock(id,warehouse,item_code,qty) VALUES ('ordinary-stock','Finished','A',1)")
        .execute(&runtime).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,barcode,qty) VALUES ('ordinary-raw','Raw','B','ORDINARY-BARCODE',1)")
        .execute(&runtime).await.unwrap();
    let stocks: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_finished_goods_stock")
        .fetch_one(&runtime)
        .await
        .unwrap();
    assert_eq!(stocks, 1);
    runtime.close().await;
    admin.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database}\""))
        .execute(&server)
        .await
        .unwrap();
    server.close().await;
}
