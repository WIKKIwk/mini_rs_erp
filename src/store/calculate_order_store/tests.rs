use rusqlite::{Connection, params};

use super::*;

#[tokio::test]
async fn multi_product_sqlite_templates_keep_width_and_distinct_layouts() {
    let dir = tempfile::tempdir().unwrap();
    let store = CalculateOrderStore::new(dir.path().join("orders.sqlite"));
    let mut template = hydrate_template_dimensions(CalculateOrderTemplate {
        products: crate::core::order_layout::tests::products(),
        customer_ref: "CUST-001".into(),
        edge_allowance_mm: 15.0,
        layers: vec![crate::core::formula::LayerInput::new("pet", "12")],
        ..Default::default()
    });
    let saved = store.upsert("admin:multi", template.clone()).await.unwrap();
    assert_eq!(saved.width_mm, 575.0);
    template.products[2].frame_product_size_mm = 90.0;
    template = hydrate_template_dimensions(template);
    let second = store.upsert("admin:multi", template).await.unwrap();
    assert_eq!(second.width_mm, 595.0);
    let loaded = store.list("admin:multi").await.unwrap();
    assert_eq!(loaded.len(), 2);
    assert!(loaded.contains(&saved));
    assert!(loaded.contains(&second));
}

#[tokio::test]
async fn calculate_order_sqlite_store_round_trips_and_upserts_templates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("orders.sqlite");
    let store = CalculateOrderStore::new(path.clone());

    let saved = store
        .upsert(
            "admin:admin",
            CalculateOrderTemplate {
                combo: None,
                products: Vec::new(),
                variant_id: String::new(),
                production_options: None,
                print_val_size_mm: None,
                id: String::new(),
                code: "Z-CPP-600".to_string(),
                name: "CPP 600".to_string(),
                saved_at: String::new(),
                order_number: "ORD-1".to_string(),
                customer_ref: "CUST-001".to_string(),
                customer: "Mijoz".to_string(),
                item_code: "ITEM-001".to_string(),
                product: "cpp / 20 mikron / 600".to_string(),
                status: String::new(),
                material_display: String::new(),
                color: String::new(),
                image_id: "img-1".to_string(),
                image_name: "rang.jpg".to_string(),
                image_mime: "image/jpeg".to_string(),
                image_size_bytes: 3,
                image_url: "/v1/mobile/calculate/orders/image/view?id=img-1".to_string(),
                frame_product_size_mm: 515.0,
                frame_count: 1.0,
                edge_allowance_mm: 15.0,
                width_mm: 530.0,
                waste_percent: 3.0,
                roll_count: Some(7),
                layers: Vec::new(),
                first_layer_material: "pet".to_string(),
                first_layer_micron: "12".to_string(),
                second_layer_material: "pe oq".to_string(),
                second_layer_micron: "30".to_string(),
                third_layer_material: String::new(),
                third_layer_micron: String::new(),
                note: String::new(),
                kg: 0.0,
                source_map_id: String::new(),
            },
        )
        .await
        .expect("save");
    let updated = store
        .upsert(
            "admin:admin",
            CalculateOrderTemplate {
                frame_product_size_mm: 615.0,
                ..saved.clone()
            },
        )
        .await
        .expect("update");

    assert_eq!(updated.id, saved.id);
    assert_eq!(updated.width_mm, 630.0);

    drop(store);
    let conn = rusqlite::Connection::open(&path).expect("open sqlite");
    let row_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM calculate_order_templates",
            [],
            |row| row.get(0),
        )
        .expect("row count");
    assert_eq!(row_count, 1);
    drop(conn);

    let reloaded = CalculateOrderStore::new(path);
    let rows = reloaded.list("admin:admin").await.expect("list");

    assert_eq!(rows, vec![updated.clone()]);
    assert!(
        serde_json::to_value(&rows[0])
            .expect("json")
            .get("kg")
            .is_none()
    );

    reloaded
        .delete("admin:admin", &updated.id)
        .await
        .expect("delete");
    assert!(reloaded.list("admin:admin").await.expect("list").is_empty());
}

#[tokio::test]
async fn calculate_order_sqlite_store_exposes_telegram_orders_to_admins() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = CalculateOrderStore::new(dir.path().join("orders.sqlite"));
    let telegram_order = store
        .upsert("telegram:123", test_template("TG-1"))
        .await
        .expect("save telegram order");

    let admin_rows = store.list("admin:admin").await.expect("admin list");
    assert_eq!(admin_rows, vec![telegram_order.clone()]);
    assert!(
        store
            .list("werka:werka")
            .await
            .expect("werka list")
            .is_empty()
    );

    store
        .delete("admin:admin", &telegram_order.id)
        .await
        .expect("delete telegram order");
    assert!(
        store
            .list("admin:admin")
            .await
            .expect("admin list")
            .is_empty()
    );
}

#[tokio::test]
async fn calculate_order_sqlite_store_exposes_telegram_images_to_admins() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = CalculateOrderStore::new(dir.path().join("orders.sqlite"));
    let saved = store
        .save_image(
            "telegram:123",
            CalculateOrderImage {
                image_id: "telegram-image".to_string(),
                image_name: "rang.jpg".to_string(),
                image_mime: "image/jpeg".to_string(),
                image_size_bytes: 0,
                body: b"telegram-image".to_vec(),
            },
        )
        .await
        .expect("save telegram image");

    assert_eq!(
        store
            .get_image("admin:admin", "telegram-image")
            .await
            .expect("admin image")
            .expect("image exists"),
        saved
    );
    assert!(
        store
            .get_image("werka:werka", "telegram-image")
            .await
            .expect("werka image")
            .is_none()
    );
}

#[tokio::test]
async fn calculate_order_sqlite_store_dedupes_same_quick_template_across_order_codes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = CalculateOrderStore::new(dir.path().join("orders.sqlite"));

    let base = CalculateOrderTemplate {
        combo: None,
        products: Vec::new(),
        variant_id: String::new(),
        production_options: None,
        print_val_size_mm: None,
        id: String::new(),
        code: "1111".to_string(),
        name: "Qurt".to_string(),
        saved_at: String::new(),
        order_number: "1111".to_string(),
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
        frame_product_size_mm: 515.0,
        frame_count: 1.0,
        edge_allowance_mm: 15.0,
        width_mm: 530.0,
        waste_percent: 5.0,
        roll_count: Some(7),
        layers: Vec::new(),
        first_layer_material: "pet".to_string(),
        first_layer_micron: "12".to_string(),
        second_layer_material: "pe oq".to_string(),
        second_layer_micron: "30".to_string(),
        third_layer_material: String::new(),
        third_layer_micron: String::new(),
        note: String::new(),
        kg: 500.0,
        source_map_id: "zakaz-1111".to_string(),
    };

    let first = store
        .upsert("admin:admin", base.clone())
        .await
        .expect("first save");
    let duplicate = CalculateOrderTemplate {
        code: "2222".to_string(),
        order_number: "2222".to_string(),
        kg: 900.0,
        source_map_id: "zakaz-2222".to_string(),
        ..base
    };
    let second = store
        .upsert("admin:admin", duplicate)
        .await
        .expect("second save");

    assert_ne!(first.code, second.code);
    assert_eq!(first.name, "Qurt");
    assert_eq!(second.name, "Qurt");
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
    assert_eq!(updated.width_mm, 640.0);
    assert_eq!(store.list("admin:admin").await.expect("list").len(), 2);
}

#[tokio::test]
async fn calculate_order_sqlite_store_round_trips_images() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("orders.sqlite");
    let store = CalculateOrderStore::new(path.clone());

    let saved = store
        .save_image(
            "admin:admin",
            CalculateOrderImage {
                image_id: "img-1".to_string(),
                image_name: " rang.jpg ".to_string(),
                image_mime: " image/jpeg ".to_string(),
                image_size_bytes: 0,
                body: b"fake-jpeg".to_vec(),
            },
        )
        .await
        .expect("save image");

    assert_eq!(saved.image_name, "rang.jpg");
    assert_eq!(saved.image_mime, "image/jpeg");
    assert_eq!(saved.image_size_bytes, 9);

    drop(store);
    let reloaded = CalculateOrderStore::new(path);
    let image = reloaded
        .get_image("admin:admin", "img-1")
        .await
        .expect("get image")
        .expect("image exists");

    assert_eq!(image, saved);
    assert!(
        reloaded
            .get_image("werka:werka", "img-1")
            .await
            .expect("get other owner")
            .is_none()
    );
}

#[tokio::test]
async fn calculate_order_sqlite_store_dedupes_legacy_same_code_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("orders.sqlite");
    let conn = Connection::open(&path).expect("open sqlite");
    conn.execute_batch(
        "CREATE TABLE calculate_order_templates (
            id TEXT PRIMARY KEY,
            owner_key TEXT NOT NULL,
            code TEXT NOT NULL,
            lower_code TEXT NOT NULL,
            name TEXT NOT NULL,
            lower_name TEXT NOT NULL,
            saved_at TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );",
    )
    .expect("legacy schema");

    let old = CalculateOrderTemplate {
        combo: None,
        products: Vec::new(),
        variant_id: String::new(),
        production_options: None,
        print_val_size_mm: None,
        id: "old-id".to_string(),
        code: "Z-DUP-1".to_string(),
        name: "Old duplicate".to_string(),
        saved_at: "100".to_string(),
        order_number: String::new(),
        customer_ref: String::new(),
        customer: String::new(),
        item_code: "ITEM-001".to_string(),
        product: "Qurt".to_string(),
        status: String::new(),
        material_display: String::new(),
        color: String::new(),
        image_id: String::new(),
        image_name: String::new(),
        image_mime: String::new(),
        image_size_bytes: 0,
        image_url: String::new(),
        frame_product_size_mm: 515.0,
        frame_count: 1.0,
        edge_allowance_mm: 15.0,
        width_mm: 530.0,
        waste_percent: 5.0,
        roll_count: Some(7),
        layers: Vec::new(),
        first_layer_material: "pet".to_string(),
        first_layer_micron: "12".to_string(),
        second_layer_material: "pe oq".to_string(),
        second_layer_micron: "30".to_string(),
        third_layer_material: String::new(),
        third_layer_micron: String::new(),
        note: String::new(),
        kg: 0.0,
        source_map_id: String::new(),
    };
    let newer = CalculateOrderTemplate {
        id: "new-id".to_string(),
        name: "New duplicate".to_string(),
        saved_at: "200".to_string(),
        frame_product_size_mm: 625.0,
        width_mm: 640.0,
        ..old.clone()
    };
    for template in [&old, &newer] {
        conn.execute(
            "INSERT INTO calculate_order_templates
                (id, owner_key, code, lower_code, name, lower_name, saved_at, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                template.id,
                "admin:admin",
                template.code,
                template.code.to_lowercase(),
                template.name,
                template.name.to_lowercase(),
                template.saved_at,
                serde_json::to_string(template).expect("json"),
            ],
        )
        .expect("insert duplicate");
    }
    drop(conn);

    let store = CalculateOrderStore::new(path.clone());
    let rows = store.list("admin:admin").await.expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "new-id");
    assert_eq!(rows[0].width_mm, 640.0);

    let conn = Connection::open(path).expect("reopen sqlite");
    let row_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM calculate_order_templates",
            [],
            |row| row.get(0),
        )
        .expect("row count");
    assert_eq!(row_count, 1);
}

#[tokio::test]
async fn calculate_order_sqlite_store_hydrates_legacy_width_only_payload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("orders.sqlite");
    let conn = Connection::open(&path).expect("open sqlite");
    conn.execute_batch(
        "CREATE TABLE calculate_order_templates (
            id TEXT PRIMARY KEY,
            owner_key TEXT NOT NULL,
            code TEXT NOT NULL,
            lower_code TEXT NOT NULL,
            name TEXT NOT NULL,
            lower_name TEXT NOT NULL,
            saved_at TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );",
    )
    .expect("legacy schema");
    let payload = serde_json::json!({
        "id": "legacy-width-only",
        "code": "Z-LEGACY",
        "name": "Legacy width",
        "saved_at": "100",
        "product": "Legacy width",
        "width_mm": 765.0,
        "edge_allowance_mm": 15.0,
        "waste_percent": 5.0,
        "first_layer_material": "pet",
        "first_layer_micron": "12",
        "second_layer_material": "pe oq",
        "second_layer_micron": "30"
    });
    conn.execute(
        "INSERT INTO calculate_order_templates
            (id, owner_key, code, lower_code, name, lower_name, saved_at, payload_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            "legacy-width-only",
            "admin:admin",
            "Z-LEGACY",
            "z-legacy",
            "Legacy width",
            "legacy width",
            "100",
            payload.to_string(),
        ],
    )
    .expect("insert legacy");
    drop(conn);

    let store = CalculateOrderStore::new(path);
    let rows = store.list("admin:admin").await.expect("list");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].frame_product_size_mm, 750.0);
    assert_eq!(rows[0].frame_count, 1.0);
    assert_eq!(rows[0].width_mm, 765.0);
}

fn test_template(code: &str) -> CalculateOrderTemplate {
    CalculateOrderTemplate {
        combo: None,
        products: Vec::new(),
        variant_id: String::new(),
        production_options: None,
        print_val_size_mm: None,
        id: String::new(),
        code: code.to_string(),
        name: "Telegram order".to_string(),
        saved_at: String::new(),
        order_number: code.to_string(),
        customer_ref: String::new(),
        customer: String::new(),
        item_code: "TG-ITEM".to_string(),
        product: "Telegram product".to_string(),
        status: String::new(),
        material_display: String::new(),
        color: String::new(),
        image_id: String::new(),
        image_name: String::new(),
        image_mime: String::new(),
        image_size_bytes: 0,
        image_url: String::new(),
        frame_product_size_mm: 515.0,
        frame_count: 1.0,
        edge_allowance_mm: 15.0,
        width_mm: 530.0,
        waste_percent: 5.0,
        roll_count: Some(1),
        layers: Vec::new(),
        first_layer_material: "pet".to_string(),
        first_layer_micron: "12".to_string(),
        second_layer_material: String::new(),
        second_layer_micron: String::new(),
        third_layer_material: String::new(),
        third_layer_micron: String::new(),
        note: String::new(),
        kg: 100.0,
        source_map_id: String::new(),
    }
}

#[tokio::test]
async fn sales_can_read_admin_templates_and_their_images_without_mutating_them() {
    let dir = tempfile::tempdir().unwrap();
    let store = CalculateOrderStore::new(dir.path().join("orders.sqlite"));
    let image = store.save_image("admin:one", CalculateOrderImage {
        image_id: "admin-linked".into(), image_name: "image.jpg".into(),
        image_mime: "image/jpeg".into(), body: vec![1, 2, 3], ..Default::default()
    }).await.unwrap();
    store.save_image("admin:one", CalculateOrderImage {
        image_id: "admin-unlinked".into(), body: vec![4], ..Default::default()
    }).await.unwrap();
    let admin = store.upsert("admin:one", CalculateOrderTemplate {
        image_id: image.image_id.clone(), item_code: "ADMIN".into(), ..test_template("ADMIN")
    }).await.unwrap();
    let own = store.upsert("sales_manager:one", CalculateOrderTemplate {
        item_code: "OWN".into(), ..test_template("OWN")
    }).await.unwrap();
    let other = store.upsert("sales_manager:two", CalculateOrderTemplate {
        item_code: "OTHER".into(), ..test_template("OTHER")
    }).await.unwrap();
    let telegram = store.upsert("telegram:one", CalculateOrderTemplate {
        item_code: "TG".into(), ..test_template("TG")
    }).await.unwrap();

    let rows = store.list("sales_manager:one").await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.contains(&admin));
    assert!(rows.contains(&own));
    assert!(!rows.contains(&other));
    assert!(!rows.contains(&telegram));
    assert_eq!(store.get_image("sales_manager:one", "admin-linked").await.unwrap(), Some(image));
    assert!(store.get_image("sales_manager:one", "admin-unlinked").await.unwrap().is_none());
    assert!(store.validate_read_identity("sales_manager:one", &admin).await.is_ok());
    assert!(matches!(store.validate_identity("sales_manager:one", &admin).await, Err(CalculateOrderError::IdentityConflict)));
    assert!(matches!(store.upsert("sales_manager:one", admin.clone()).await, Err(CalculateOrderError::IdentityConflict)));
    store.delete("sales_manager:one", &admin.id).await.unwrap();
    assert!(store.list("admin:one").await.unwrap().contains(&admin));
    assert!(matches!(store.validate_read_identity("sales_manager:one", &other).await, Err(CalculateOrderError::IdentityConflict)));
}
