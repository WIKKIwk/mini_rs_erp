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
    let sink = PostgresMiniOrderSink::new(pool.clone());

    sink.save_order(&test_map(), &test_template())
        .await
        .expect("save mini order");

    let order_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mini_orders")
        .fetch_one(&pool)
        .await
        .expect("count orders");
    let product_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mini_order_products")
        .fetch_one(&pool)
        .await
        .expect("count products");
    assert_eq!(order_count, 1);
    assert_eq!(product_count, 1);

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
        roll_count: Some(7.0),
        width_mm: Some(650.0),
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
        status: String::new(),
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
        width_mm: 650.0,
        waste_percent: 5.0,
        roll_count: Some(7.0),
        first_layer_material: "pet".to_string(),
        first_layer_micron: "12".to_string(),
        second_layer_material: "pe oq".to_string(),
        second_layer_micron: "30".to_string(),
        third_layer_material: String::new(),
        third_layer_micron: String::new(),
        note: "test".to_string(),
        kg: 500.0,
        source_map_id: "zakaz-9001".to_string(),
    }
}
