use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::mini_orders::MiniOrderSink;
use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapEdge, ProductionMapNode, ProductionMapNodeKind,
};
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_mini_order::PostgresMiniOrderSink;

#[tokio::test]
async fn postgres_print_side_updates_closed_order_and_remembers_its_layout() {
    use crate::core::production_map::ProductionMapStorePort;
    use crate::db::postgres_production_map::PostgresProductionMapStore;

    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let pool = sqlx::postgres::PgPoolOptions::new().max_connections(1)
        .connect(&url).await.unwrap();
    sqlx::raw_sql(
        "CREATE TEMP TABLE mini_orders (
             id text PRIMARY KEY, customer_ref text, product_code text, calculation_json jsonb
         );
         CREATE TEMP TABLE mini_production_maps (
             id text PRIMARY KEY, map_json jsonb, created_at timestamptz DEFAULT now(),
             lifecycle_status text DEFAULT 'closed'
         );",
    ).execute(&pool).await.unwrap();
    let migration = include_str!("../../../migrations/postgres/0194_order_print_side_preferences.sql");
    sqlx::raw_sql(&migration.replacen("CREATE TABLE", "CREATE TEMP TABLE", 1))
        .execute(&pool).await.unwrap();

    let cut = "apparatus:default:asset-010";
    let mut map = test_map();
    let mut node = test_node("cut", ProductionMapNodeKind::Apparatus, "Rezka", 60.0);
    node.apparatus_id = cut.into();
    map.nodes.insert(1, node);
    map.edges[0].to = "cut".into();
    map.edges.push(ProductionMapEdge { from: "cut".into(), to: "end".into(), branch: String::new() });
    let mut template = test_template();
    template.customer_ref = "customer-a".into();
    template.item_code = map.product_code.clone();
    sqlx::query("INSERT INTO mini_orders VALUES ($1,$2,$3,$4), ('zakaz-8999',$2,$3,$4)")
        .bind(&map.id).bind(&template.customer_ref).bind(&template.item_code)
        .bind(serde_json::to_value(&template).unwrap())
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_production_maps (id,map_json) VALUES ($1,$2)")
        .bind(&map.id).bind(serde_json::to_value(&map).unwrap())
        .execute(&pool).await.unwrap();

    sqlx::raw_sql(
        "INSERT INTO mini_production_maps (id, map_json, created_at) VALUES
         ('zakaz-8999', '{\"standard_print_side\":false}', now() - interval '1 day'),
         ('zakaz-9002', '{\"standard_print_side\":null}', now()),
         ('zakaz-9003', '{\"standard_print_side\":true}', now()),
         ('template-9001', '{}', now()),
         ('zakaz-draft-9001', '{}', now());",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/postgres/0195_backfill_order_print_side.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();

    let stored: Vec<(String, Option<bool>, bool)> = sqlx::query_as(
        "SELECT id, (map_json->>'standard_print_side')::boolean, standard_print_side_defaulted
         FROM mini_production_maps ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        stored,
        [
            ("template-9001".to_string(), None, false),
            ("zakaz-8999".to_string(), Some(false), false),
            ("zakaz-9001".to_string(), Some(true), true),
            ("zakaz-9002".to_string(), Some(true), true),
            ("zakaz-9003".to_string(), Some(true), false),
            ("zakaz-draft-9001".to_string(), None, false),
        ]
    );

    let sink = PostgresMiniOrderSink::new(pool.clone());
    assert_eq!(sink.print_side_preference(&template).await.unwrap(), Some(false));
    let store = PostgresProductionMapStore::new(pool.clone());
    store.set_order_print_side(&map.id, &[cut.into()], true, true).await.unwrap();
    assert_eq!(sink.print_side_preference(&template).await.unwrap(), Some(true));
    store.set_order_print_side(&map.id, &[cut.into()], false, true).await.unwrap();
    let (stored, lifecycle, defaulted): (serde_json::Value, String, bool) =
        sqlx::query_as(
            "SELECT map_json, lifecycle_status, standard_print_side_defaulted
             FROM mini_production_maps WHERE id=$1",
        )
        .bind(&map.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    map.standard_print_side = Some(false);
    assert_eq!(stored, serde_json::to_value(&map).unwrap());
    assert_eq!(lifecycle, "closed");
    assert!(!defaulted);
    template.kg += 100.0;
    assert_eq!(sink.print_side_preference(&template).await.unwrap(), Some(false));
    template.customer_ref = "customer-b".into();
    assert_eq!(sink.print_side_preference(&template).await.unwrap(), None);
    template.customer_ref = "customer-a".into();
    template.frame_count += 1.0;
    assert_eq!(sink.print_side_preference(&template).await.unwrap(), None);
}

#[tokio::test]
async fn postgres_order_materials_read_saved_layers_in_one_query() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    // One connection keeps the fixture in temporary tables, isolated from
    // both live data and other tests, while exercising the real SQL reader.
    let pool = sqlx::postgres::PgPoolOptions::new().max_connections(1)
        .connect(&url).await.unwrap();
    sqlx::raw_sql("CREATE TEMP TABLE mini_order_products (order_id text, layers_json jsonb);
        CREATE TEMP TABLE mini_calculate_materials (id text, payload_json jsonb);
        CREATE TEMP TABLE mini_items (code text, name text, uom text, item_group text);
        INSERT INTO mini_items VALUES
            ('FILM-PET', 'PET', 'Kg', 'Rulon'),
            ('OLD-PE', 'PE white', 'Kg', 'Rulon'),
            ('OPP', 'OPP', 'Kg', 'Rulon');
        INSERT INTO mini_calculate_materials VALUES ('pe-id', '{\"name\":\"PE white\"}');
        INSERT INTO mini_order_products VALUES
            ('zakaz-9001', '[{\"material\":\"pet\",\"micron\":\"12\"},
                            {\"material\":\"PET\",\"micron\":\"20\"},
                            {\"material\":\"PET\",\"micron\":\"12\"},
                            {\"material_id\":\"pe-id\",\"material\":\"PE oq\",\"micron\":\"30\"}]'),
            ('zakaz-9002', '[{\"material\":\"OPP\",\"micron\":\"25\"}]'),
            ('zakaz-9003', '[{\"material\":\"OPP\",\"micron\":\"25\"}]');")
        .execute(&pool).await.unwrap();
    let sink = PostgresMiniOrderSink::new(pool.clone());
    let materials = sink.order_materials("zakaz-9001").await.unwrap();
    assert_eq!(materials.len(), 2);
    assert_eq!(materials[0].item.code, "FILM-PET");
    assert_eq!(materials[0].microns, vec![12.0, 20.0]);
    assert_eq!(materials[1].item.code, "OLD-PE");
    assert_eq!(materials[1].microns, vec![30.0]);
    assert_eq!(sink.order_materials("zakaz-9002").await.unwrap()[0].item.code, "OPP");
    assert!(sink.order_materials("zakaz-missing").await.unwrap().is_empty());
    sqlx::raw_sql("CREATE TEMP TABLE mini_raw_material_assignments
            (barcode text, order_id text, item_code text);
        CREATE TEMP TABLE mini_raw_material_stock
            (barcode text, item_code text, micron numeric, reserved_order_id text, status text, qty numeric);
        INSERT INTO mini_raw_material_assignments VALUES
            ('pet12', 'zakaz-9001', 'FILM-PET'),
            ('pet20-other', 'zakaz-9002', 'FILM-PET'),
            ('pe-wrong', 'zakaz-9001', 'OLD-PE');
        INSERT INTO mini_raw_material_stock VALUES
            ('pet12', 'FILM-PET', 12, '', 'available', 0.001),
            ('pet20-other', 'FILM-PET', 20, 'zakaz-9002', 'reserved', 15),
            ('pe-wrong', 'OLD-PE', 50, 'zakaz-9001', 'reserved', 15);")
        .execute(&pool).await.unwrap();
    let ids = vec!["zakaz-9001".into(), "zakaz-9002".into(), "zakaz-9003".into()];
    let groups = vec!["Rulon".into()];
    let tasks = sink.order_material_tasks(&ids, &groups).await.unwrap();
    assert_eq!(tasks.len(), 5);
    assert!(tasks[..4].iter().all(|task| task.assigned));
    // A tiny available roll already removes the whole order, even when other
    // layers are missing or the linked material has a different type/micron.
    assert!(!tasks[4].assigned); // Only the order with no assignment is a task.
    assert!(sink.order_material_tasks(&ids, &["Kraska".into()]).await.unwrap().is_empty());
    for (status, qty) in [("consumed", 0), ("available", 15),
                          ("reserved", 0), ("in_use", 5)] {
        sqlx::query("UPDATE mini_raw_material_stock SET status=$1, qty=$2 WHERE barcode='pet12'")
            .bind(status).bind(qty).execute(&pool).await.unwrap();
        assert!(sink.order_material_tasks(&ids, &groups).await.unwrap()[..3]
            .iter().all(|task| task.assigned));
    }
    sqlx::query("DELETE FROM mini_raw_material_assignments WHERE order_id='zakaz-9001'")
        .execute(&pool).await.unwrap();
    let tasks = sink.order_material_tasks(&ids, &groups).await.unwrap();
    assert!(tasks[..3].iter().all(|task| !task.assigned));
    assert!(tasks[3].assigned);
    assert!(!tasks[4].assigned); // Another order's link does not hide this order.
    pool.close().await;
}

#[tokio::test]
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

    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, db_name))
        .await
        .expect("test db");
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
                 $6::bigint,
                 ($7::double precision)::numeric(18,6), $8)",
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
    assert_eq!(order_status, "draft");
    assert_eq!(product_form, "rulon");
    assert_eq!(kg, "500.123457");
    assert_eq!(width_mm, "650.000030");
    assert_eq!(roll_count, "7");

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
        combo: None,
        products: Vec::new(),
        print_val_size_mm: None,
        id: "zakaz-9001".to_string(),
        product_code: "ITEM-9001".to_string(),
        title: "Mini order map".to_string(),
        code: "9001".to_string(),
        order_number: "9001".to_string(),
        customer_name: String::new(),
        image_id: String::new(),
        roll_count: Some(7),
        width_mm: Some(650.00003),
        order_kg: None,
        base_length: None,
        duplicate_rezka_labels: None,
        include_bobina_in_net: None,
        standard_print_side: None,
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
        apparatus_id: String::new(),
        formula: None,
        role_code: String::new(),
        item_code: String::new(),
        qty_formula: String::new(),
        from_location: String::new(),
        to_location: String::new(),
        alternative_group_id: String::new(),
        alternative_group_label: String::new(),
        alternative_assigned_title: String::new(),
        alternative_assigned_apparatus_id: String::new(),
        rezka_kadr_count: None,
        rezka_frame_groups: Vec::new(),
        rezka_label_length: None,
        x: 0.0,
        y,
    }
}

fn test_template() -> CalculateOrderTemplate {
    CalculateOrderTemplate {
        combo: None,
        products: Vec::new(),
        variant_id: String::new(),
        production_options: None,
        print_val_size_mm: None,
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
        roll_count: Some(7),
        layers: Vec::new(),
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
