use rusqlite::{Connection, params};

use super::*;

#[tokio::test]
async fn calculate_order_sqlite_store_round_trips_and_upserts_templates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("orders.sqlite");
    let store = CalculateOrderStore::new(path.clone());

    let saved = store
        .upsert(
            "admin:admin",
            CalculateOrderTemplate {
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
                roll_count: Some(7.0),
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
async fn calculate_order_sqlite_store_dedupes_same_quick_template_across_order_codes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = CalculateOrderStore::new(dir.path().join("orders.sqlite"));

    let base = CalculateOrderTemplate {
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
        roll_count: Some(7.0),
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
        roll_count: Some(7.0),
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
