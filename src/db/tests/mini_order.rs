use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::mini_orders::MiniOrderSink;
use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapEdge, ProductionMapNode, ProductionMapNodeKind,
};
use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_mini_order::PostgresMiniOrderSink;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_mini_orders"]
async fn postgres_mini_order_sink_saves_order_and_product_rows() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_mini_orders";
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
    apply_foundation_migration(&pool)
        .await
        .expect("migration is idempotent");
    let sink = PostgresMiniOrderSink::new(pool.clone());
    let map = test_map();
    let map_json = serde_json::to_value(&map).expect("serialize map");
    sqlx::query(
        "INSERT INTO mini_production_maps
            (id, product_code, title, code, order_number, roll_count, width_mm, map_json)
         VALUES ($1, $2, $3, $4, $5,
                 ($6::double precision)::numeric(24,9),
                 ($7::double precision)::numeric(24,9), $8)",
    )
    .bind(&map.id)
    .bind(&map.product_code)
    .bind(&map.title)
    .bind(&map.code)
    .bind(&map.order_number)
    .bind(map.roll_count)
    .bind(map.width_mm)
    .bind(map_json)
    .execute(&pool)
    .await
    .expect("insert production map");

    sink.save_order(&map, &test_template())
        .await
        .expect("save mini order");
    sink.save_order(&map, &test_template())
        .await
        .expect("save mini order idempotently");

    let order_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mini_orders")
        .fetch_one(&pool)
        .await
        .expect("count orders");
    let product_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mini_order_products")
        .fetch_one(&pool)
        .await
        .expect("count products");
    let linked_order_id: Option<String> =
        sqlx::query_scalar("SELECT order_id FROM mini_production_maps WHERE id = $1")
            .bind(&map.id)
            .fetch_one(&pool)
            .await
            .expect("read production map order link");
    let migration_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mini_schema_migrations")
        .fetch_one(&pool)
        .await
        .expect("count applied migrations");
    let (order_status, product_form, kg, width_mm, roll_count): (
        String,
        String,
        String,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT status, product_form, kg::text, width_mm::text, roll_count::text
         FROM mini_orders WHERE id = $1",
    )
    .bind(&map.id)
    .fetch_one(&pool)
    .await
    .expect("read order semantics and quantities");
    assert_eq!(order_count, 1);
    assert_eq!(product_count, 1);
    assert_eq!(linked_order_id.as_deref(), Some("zakaz-9001"));
    assert_eq!(migration_count, 4);
    assert_eq!(order_status, "draft");
    assert_eq!(product_form, "rulon");
    assert_eq!(kg, "500.123456789");
    assert_eq!(width_mm, "650.000030000");
    assert_eq!(roll_count, "7.000000123");

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

fn test_map() -> ProductionMapDefinition {
    ProductionMapDefinition {
        id: "zakaz-9001".to_string(),
        product_code: "ITEM-9001".to_string(),
        title: "Mini order map".to_string(),
        code: "9001".to_string(),
        order_number: "9001".to_string(),
        roll_count: Some(7.000000123),
        width_mm: Some(650.00003),
        order_kg: None,
        base_length: None,
        nodes: vec![
            test_node("start", ProductionMapNodeKind::Start, "Start", 0.0),
            test_node("end", ProductionMapNodeKind::End, "End", 120.0),
        ],
        edges: vec![ProductionMapEdge {
            from: "start".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        }],
    }
}

fn test_node(id: &str, kind: ProductionMapNodeKind, title: &str, y: f64) -> ProductionMapNode {
    ProductionMapNode {
        id: id.to_string(),
        kind,
        title: title.to_string(),
        formula: None,
        role_code: String::new(),
        item_code: String::new(),
        qty_formula: String::new(),
        from_location: String::new(),
        to_location: String::new(),
        alternative_group_id: String::new(),
        alternative_group_label: String::new(),
        alternative_assigned_title: String::new(),
        rezka_kadr_count: None,
        rezka_label_length: None,
        x: 0.0,
        y,
    }
}

fn test_template() -> CalculateOrderTemplate {
    CalculateOrderTemplate {
        id: String::new(),
        code: "9001".to_string(),
        name: "Mini mahsulot".to_string(),
        saved_at: String::new(),
        order_number: "9001".to_string(),
        customer_ref: "CUST-9001".to_string(),
        customer: "Mijoz".to_string(),
        item_code: "ITEM-9001".to_string(),
        product: "Mini mahsulot".to_string(),
        status: "rulon".to_string(),
        material_display: "PET / PE".to_string(),
        color: "oq".to_string(),
        image_id: String::new(),
        image_name: String::new(),
        image_mime: String::new(),
        image_size_bytes: 0,
        image_url: String::new(),
        frame_product_size_mm: 635.0,
        frame_count: 1.0,
        edge_allowance_mm: 15.0,
        width_mm: 650.00003,
        waste_percent: 5.0,
        roll_count: Some(7.000000123),
        first_layer_material: "pet".to_string(),
        first_layer_micron: "12".to_string(),
        second_layer_material: "pe oq".to_string(),
        second_layer_micron: "30".to_string(),
        third_layer_material: String::new(),
        third_layer_micron: String::new(),
        note: "test".to_string(),
        kg: 500.123456789,
        source_map_id: "zakaz-9001".to_string(),
    }
}
