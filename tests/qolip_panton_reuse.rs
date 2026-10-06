use mini_rs_erp::core::qolip::{QolipError, QolipProductSpec, QolipStorePort};
use mini_rs_erp::db::postgres::apply_postgres_migrations_through_version;
use mini_rs_erp::db::postgres_qolip::PostgresQolipStore;
use sqlx::{PgPool, postgres::PgConnectOptions};

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn panton_colors_are_reusable_after_all_global_numbers_are_used() {
    let admin_url =
        std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").expect("disposable PostgreSQL admin URL");
    let database = format!("qolip_panton_reuse_{:032x}", rand::random::<u128>());
    let admin_pool = PgPool::connect(&admin_url).await.expect("admin database");
    sqlx::query(&format!("CREATE DATABASE \"{database}\""))
        .execute(&admin_pool)
        .await
        .expect("create isolated database");
    let options = admin_url
        .parse::<PgConnectOptions>()
        .expect("database options")
        .database(&database);
    let pool = PgPool::connect_with(options)
        .await
        .expect("isolated database");
    // Later production-map migrations require factory equipment fixtures.
    // Apply the baseline and the current qolip set constraints for this test.
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .expect("baseline migrations");
    for migration in [
        include_str!("../migrations/postgres/0136_qolip_alternative_sets.sql"),
        include_str!("../migrations/postgres/0137_qolip_set_integrity.sql"),
    ] {
        sqlx::raw_sql(migration)
            .execute(&pool)
            .await
            .expect("production qolip schema");
    }
    let store = PostgresQolipStore::new(pool.clone());

    store
        .put_product_specs(
            (1..=34)
                .map(|number| {
                    spec(
                        "old-product",
                        "old-set",
                        &format!("OLD-{number}"),
                        &format!("Panton {number}"),
                    )
                })
                .collect(),
        )
        .await
        .expect("existing Panton specs");
    sqlx::query(
        "INSERT INTO mini_qolip_locations (
             id, block, warehouse, item_code, item_name, qolip_code, size, quantity, payload_json
         ) SELECT 'location-' || n, 'A', 'Molds', 'old-product', 'Old product',
             'OLD-' || n, 40, 1, jsonb_build_object('color', 'Panton ' || n)
         FROM generate_series(35, 67) n",
    )
    .execute(&pool)
    .await
    .expect("existing Panton locations");
    sqlx::query(
        "INSERT INTO mini_qolip_checkouts (
             id, location_id, block, warehouse, item_code, item_name, qolip_code,
             size, quantity, issued_to_ref, issued_to_name, status, payload_json
         ) SELECT 'checkout-' || n, 'source-' || n, 'A', 'Molds', 'old-product',
             'Old product', 'OLD-' || n, 40, 1, 'worker', 'Worker', 'returned',
             jsonb_build_object('color', 'Panton ' || n)
         FROM generate_series(68, 100) n",
    )
    .execute(&pool)
    .await
    .expect("returned Panton checkouts");
    let used: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT color) FROM (
             SELECT payload_json->>'color' AS color FROM mini_qolip_product_specs
             UNION ALL
             SELECT payload_json->>'color' FROM mini_qolip_locations
             UNION ALL
             SELECT payload_json->>'color' FROM mini_qolip_checkouts
         ) colors",
    )
    .fetch_one(&pool)
    .await
    .expect("global color precondition");
    assert_eq!(used, 100);

    let first = store
        .put_product_spec(spec("new-product", "set-a", "NEW-A1", " PANTON 1 "))
        .await
        .expect("reuse Panton 1 despite occupied global numbers");
    assert_eq!(first.color, "PANTON 1");
    let batch = store
        .put_product_specs(vec![
            spec("new-product", "set-b", "NEW-B1", "PANTON 1"),
            spec("new-product", "set-b", "NEW-B2", "PANTON 2"),
        ])
        .await
        .expect("another set starts from Panton 1");
    assert_eq!(batch[0].color, "PANTON 1");
    assert_eq!(batch[1].color, "PANTON 2");
    assert_ne!(first.set_id(), batch[0].set_id());

    let edited = store
        .rename_product_spec(
            "NEW-A1",
            QolipProductSpec {
                color: "PANTON 2".into(),
                ..first
            },
        )
        .await
        .expect("edit keeps the selected Panton number");
    assert_eq!(edited.color, "PANTON 2");
    let loaded = store
        .product_spec_by_qolip_code("NEW-A1")
        .await
        .expect("read saved color")
        .expect("saved spec");
    assert_eq!(loaded.color, "PANTON 2");
    assert_eq!(loaded.set_id(), "set-a");
    assert_eq!(
        store
            .put_product_spec(spec("new-product", "set-c", "new-a1", "PANTON 1"))
            .await,
        Err(QolipError::QolipCodeConflict),
    );

    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database}\" WITH (FORCE)"))
        .execute(&admin_pool)
        .await
        .expect("drop isolated database");
    admin_pool.close().await;
}

fn spec(item: &str, set: &str, code: &str, color: &str) -> QolipProductSpec {
    QolipProductSpec {
        item_code: item.into(),
        item_name: item.into(),
        item_group: "Products".into(),
        warehouse: "Molds".into(),
        qolip_set_id: set.into(),
        qolip_code: code.into(),
        size: 40,
        color: color.into(),
        created_by_role: "qolipchi".into(),
        created_by_ref: "clerk".into(),
        created_by_name: "Clerk".into(),
    }
}
