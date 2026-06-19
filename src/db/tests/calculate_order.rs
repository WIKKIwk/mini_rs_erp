use crate::core::calculate_orders::{
    CalculateOrderImage, CalculateOrderStorePort, CalculateOrderTemplate,
};
use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_calculate_order::PostgresCalculateOrderStore;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_calculate_orders"]
async fn postgres_calculate_order_store_round_trips_and_dedupes_quick_templates() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_calculate_orders";
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let test_url = format!("postgres://wikki@127.0.0.1:5432/{db_name}");
    let pool = sqlx::PgPool::connect(&test_url).await.expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    let store = PostgresCalculateOrderStore::new(pool.clone());

    let first = store
        .upsert("admin:admin", test_template("1111", 530.0, 500.0))
        .await
        .expect("save first");
    let second = store
        .upsert("admin:admin", test_template("2222", 530.0, 900.0))
        .await
        .expect("save duplicate quick template");
    assert_ne!(first.code, second.code);

    let rows = store.list("admin:admin").await.expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].code, second.code);

    let updated = store
        .upsert(
            "admin:admin",
            CalculateOrderTemplate {
                frame_product_size_mm: 625.0,
                ..second.clone()
            },
        )
        .await
        .expect("update second");
    assert_eq!(updated.id, second.id);

    let rows = store.list("admin:admin").await.expect("list after update");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| row.code == first.code));
    assert!(rows.iter().any(|row| row.code == updated.code));

    store
        .delete("admin:admin", &updated.id)
        .await
        .expect("delete updated");
    let rows = store.list("admin:admin").await.expect("list after delete");
    assert_eq!(rows, vec![first]);

    let saved_image = store
        .save_image(
            "admin:admin",
            CalculateOrderImage {
                image_id: "img-1".to_string(),
                image_name: "rang.jpg".to_string(),
                image_mime: "image/jpeg".to_string(),
                image_size_bytes: 0,
                body: b"fake-jpeg".to_vec(),
            },
        )
        .await
        .expect("save image");
    let loaded_image = store
        .get_image("admin:admin", "img-1")
        .await
        .expect("get image")
        .expect("image exists");
    assert_eq!(loaded_image, saved_image);
    assert!(
        store
            .get_image("werka:werka", "img-1")
            .await
            .expect("other owner")
            .is_none()
    );

    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("cleanup test db");
    admin_pool.close().await;
}

fn test_template(code: &str, width_mm: f64, kg: f64) -> CalculateOrderTemplate {
    CalculateOrderTemplate {
        id: String::new(),
        code: code.to_string(),
        name: "Qurt".to_string(),
        saved_at: String::new(),
        order_number: code.to_string(),
        customer_ref: String::new(),
        customer: String::new(),
        item_code: "QURT-001".to_string(),
        product: "Qurt".to_string(),
        status: String::new(),
        material_display: String::new(),
        color: String::new(),
        image_id: String::new(),
        image_name: String::new(),
        image_mime: String::new(),
        image_size_bytes: 0,
        image_url: String::new(),
        frame_product_size_mm: width_mm - 15.0,
        frame_count: 1.0,
        edge_allowance_mm: 15.0,
        width_mm,
        waste_percent: 5.0,
        roll_count: Some(7.0),
        first_layer_material: "pet".to_string(),
        first_layer_micron: "12".to_string(),
        second_layer_material: "pe oq".to_string(),
        second_layer_micron: "30".to_string(),
        third_layer_material: String::new(),
        third_layer_micron: String::new(),
        note: String::new(),
        kg,
        source_map_id: format!("zakaz-{code}"),
    }
}
