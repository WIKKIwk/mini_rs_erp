use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::core::admin::item_customer_policy::FINISHED_GOODS_CUSTOMER_REQUIRED;
use crate::core::admin::ports::{AdminPortError, AdminReadPort, AdminWritePort};
use crate::db::postgres::apply_postgres_migrations_through;
use crate::db::postgres_admin_catalog::PostgresAdminCatalogStore;
use crate::db::postgres_customer::PostgresCustomerStore;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_admin_item_update"]
async fn postgres_admin_item_update_preserves_details_and_live_references() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".to_string());
    let db_name = "mini_rs_erp_test_admin_item_update";
    let admin_options = PgConnectOptions::from_str(&admin_url).expect("admin database url");
    let admin_pool = PgPoolOptions::new()
        .connect_with(admin_options.clone())
        .await
        .expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop stale test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");

    let pool = PgPoolOptions::new()
        .connect_with(admin_options.database(db_name))
        .await
        .expect("test db");
    apply_postgres_migrations_through(&pool, 15)
        .await
        .expect("apply migrations before item warehouse removal");
    sqlx::query(
        "INSERT INTO mini_items
             (code, name, uom, warehouse, item_group, payload_json)
         VALUES
             ('LEGACY-WAREHOUSE-ITEM', 'Legacy warehouse item', 'Kg', 'Stores - A',
              'All Item Groups',
              '{\"code\":\"LEGACY-WAREHOUSE-ITEM\",\"name\":\"Legacy warehouse item\",\"uom\":\"Kg\",\"warehouse\":\"Stores - A\",\"item_group\":\"All Item Groups\"}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("seed legacy item warehouse");
    apply_postgres_migrations_through(&pool, 16)
        .await
        .expect("remove item warehouse ownership");
    let warehouse_column_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1
             FROM information_schema.columns
             WHERE table_schema = 'public'
               AND table_name = 'mini_items'
               AND column_name = 'warehouse'
         )",
    )
    .fetch_one(&pool)
    .await
    .expect("item warehouse column lookup");
    assert!(!warehouse_column_exists);
    let payload_has_warehouse: bool = sqlx::query_scalar(
        "SELECT payload_json ? 'warehouse'
         FROM mini_items
         WHERE code = 'LEGACY-WAREHOUSE-ITEM'",
    )
    .fetch_one(&pool)
    .await
    .expect("legacy item payload");
    assert!(!payload_has_warehouse);
    let store = PostgresAdminCatalogStore::new(pool.clone());
    store
        .create_item_group("Tayyor mahsulot", "All Item Groups", true)
        .await
        .expect("finished goods group");
    store
        .create_item_group("Tayyor mahsulot / Paket", "Tayyor mahsulot", true)
        .await
        .expect("finished goods child group");
    sqlx::query(
        "INSERT INTO mini_customers (ref, name, phone, payload_json)
         VALUES ('CUST-001', 'Customer One', '+998900000001', '{}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("seed customer");
    store
        .create_item_with_customer(
            "ITEM-OLD",
            "Old item name",
            "Kg",
            "Tayyor mahsulot / Paket",
            Some("CUST-001"),
        )
        .await
        .expect("create item");
    let duplicate_error = store
        .create_item_with_customer(
            "item-old",
            "Must not replace old item",
            "Dona",
            "All Item Groups",
            Some("CUST-001"),
        )
        .await
        .expect_err("duplicate item create");
    assert!(matches!(
        duplicate_error,
        AdminPortError::InvalidInput(message) if message == "item code already exists"
    ));
    let unchanged = store.item_detail("ITEM-OLD").await.expect("unchanged item");
    assert_eq!(unchanged.name, "Old item name");
    assert_eq!(unchanged.uom, "Kg");
    assert_eq!(unchanged.item_group, "Tayyor mahsulot / Paket");
    let before = store.item_detail("ITEM-OLD").await.expect("item detail");
    assert!(before.is_finished_goods);
    assert_eq!(before.customers.len(), 1);

    let missing_customer_error = store
        .create_item(
            "FINISHED-WITHOUT-CUSTOMER",
            "Must roll back",
            "Kg",
            "Tayyor mahsulot / Paket",
        )
        .await
        .expect_err("finished item without customer");
    assert!(matches!(
        missing_customer_error,
        AdminPortError::InvalidInput(message) if message == FINISHED_GOODS_CUSTOMER_REQUIRED
    ));
    let rolled_back: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS (
             SELECT 1 FROM mini_items WHERE code = 'FINISHED-WITHOUT-CUSTOMER'
         )",
    )
    .fetch_one(&pool)
    .await
    .expect("rolled back item lookup");
    assert!(rolled_back);

    sqlx::raw_sql(
        "INSERT INTO mini_gscale_receipts
             (name, status, item_code, warehouse, qty, uom, barcode, payload_json)
         VALUES
             ('MAT-DRAFT', 'draft', 'ITEM-OLD', 'Homashyo ombori', 5, 'kg',
              'BARCODE-001', '{\"item_code\":\"ITEM-OLD\",\"item_name\":\"Old item name\"}'::jsonb);
         INSERT INTO mini_raw_material_stock
             (id, warehouse, item_code, item_name, barcode, qty, uom, status, payload_json)
         VALUES
             ('RAW-001', 'Homashyo ombori', 'ITEM-OLD', 'Old item name',
              'BARCODE-001', 5, 'kg', 'available',
              '{\"item_code\":\"ITEM-OLD\",\"item_name\":\"Old item name\"}'::jsonb);
         INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ('ORDER-001', 'PRODUCT-001', 'Order One', '{}'::jsonb);
         INSERT INTO mini_raw_material_assignments
             (barcode, order_id, apparatus, item_code, item_group, payload_json)
         VALUES
             ('BARCODE-001', 'ORDER-001', 'Apparatus One', 'ITEM-OLD',
              'Tayyor mahsulot / Paket',
              '{\"item_code\":\"ITEM-OLD\",\"item_name\":\"Old item name\"}'::jsonb);
         INSERT INTO mini_finished_goods_stock
             (id, warehouse, item_code, item_name, qty, uom, status, payload_json)
         VALUES
             ('FINISHED-001', 'Tayyor mahsulot ombori', 'ITEM-OLD', 'Old item name',
              2, 'dona', 'available',
              '{\"item_code\":\"ITEM-OLD\",\"item_name\":\"Old item name\"}'::jsonb);
         INSERT INTO mini_qolip_locations
             (id, block, item_code, item_name, qolip_code, size, quantity, payload_json)
         VALUES
             ('LOC-001', 'A', 'ITEM-OLD', 'Old item name', 'Q-001', 10, 1,
              '{\"item_code\":\"ITEM-OLD\",\"item_name\":\"Old item name\"}'::jsonb);
         INSERT INTO mini_qolip_product_specs
             (item_code, item_name, item_group, qolip_code, size, payload_json)
         VALUES
             ('ITEM-OLD', 'Old item name', 'Tayyor mahsulot / Paket', 'Q-001', 10,
              '{\"item_code\":\"ITEM-OLD\",\"item_name\":\"Old item name\"}'::jsonb);
         INSERT INTO mini_qolip_checkouts
             (id, location_id, block, item_code, item_name, qolip_code, size, quantity,
              issued_to_ref, issued_to_name, status, payload_json)
         VALUES
             ('CHECKOUT-001', 'LOC-001', 'A', 'ITEM-OLD', 'Old item name', 'Q-001',
              10, 1, 'WORKER-001', 'Worker One', 'open',
              '{\"item_code\":\"ITEM-OLD\",\"item_name\":\"Old item name\"}'::jsonb);
         INSERT INTO mini_quick_order_templates
             (id, owner_key, code, name, item_code, product_name, payload_json, quick_key)
         VALUES
             ('TEMPLATE-001', 'admin:ADMIN-001', 'TMP-001', 'Template One',
              'ITEM-OLD', 'Old item name',
              '{\"item_code\":\"ITEM-OLD\",\"product_name\":\"Old item name\"}'::jsonb,
              'admin:ADMIN-001:TMP-001');
         INSERT INTO mini_rps_batches
             (owner_key, batch_id, active, owner_role, owner_ref, item_code, payload_json)
         VALUES
             ('admin:ADMIN-001', 'BATCH-001', true, 'admin', 'ADMIN-001', 'ITEM-OLD',
              '{\"item_code\":\"ITEM-OLD\"}'::jsonb);",
    )
    .execute(&pool)
    .await
    .expect("seed live item references");

    let updated = store
        .update_item("ITEM-OLD", "ITEM-NEW", "New item name")
        .await
        .expect("update item");

    assert_eq!(updated.code, "ITEM-NEW");
    assert_eq!(updated.name, "New item name");
    assert_eq!(updated.created_at_unix, before.created_at_unix);
    assert!(updated.updated_at_unix >= before.updated_at_unix);
    assert!(updated.is_finished_goods);
    assert_eq!(updated.customers.len(), 1);
    assert_eq!(updated.customers[0].ref_, "CUST-001");
    assert!(matches!(
        store.item_detail("ITEM-OLD").await,
        Err(AdminPortError::NotFound)
    ));

    let customer_item: String = sqlx::query_scalar(
        "SELECT item_code FROM mini_customer_items WHERE customer_ref = 'CUST-001'",
    )
    .fetch_one(&pool)
    .await
    .expect("customer item code");
    assert_eq!(customer_item, "ITEM-NEW");

    let customer_store = PostgresCustomerStore::new(pool.clone());
    let unlink_error = customer_store
        .unassign_customer_item_guarded("CUST-001", "ITEM-NEW")
        .await
        .expect_err("last customer must stay linked");
    assert!(matches!(
        unlink_error,
        AdminPortError::InvalidInput(message) if message == FINISHED_GOODS_CUSTOMER_REQUIRED
    ));
    let customer_link_still_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM mini_customer_items
             WHERE customer_ref = 'CUST-001' AND item_code = 'ITEM-NEW'
         )",
    )
    .fetch_one(&pool)
    .await
    .expect("customer link after rejected unlink");
    assert!(customer_link_still_exists);

    store
        .create_item("ORDINARY-001", "Ordinary", "Kg", "All Item Groups")
        .await
        .expect("ordinary item");
    let bulk_error = store
        .update_item_groups_bulk(&["ORDINARY-001".to_string()], "Tayyor mahsulot / Paket")
        .await
        .expect_err("customerless bulk reclassification");
    assert!(matches!(
        bulk_error,
        AdminPortError::InvalidInput(message) if message == FINISHED_GOODS_CUSTOMER_REQUIRED
    ));
    assert_eq!(
        store
            .item_detail("ORDINARY-001")
            .await
            .expect("ordinary detail")
            .item_group,
        "All Item Groups"
    );

    store
        .create_item_group("Candidate", "All Item Groups", true)
        .await
        .expect("candidate group");
    store
        .create_item("CANDIDATE-001", "Candidate", "Kg", "Candidate")
        .await
        .expect("candidate item");
    let parent_error = store
        .move_item_group_parent("Candidate", "Tayyor mahsulot")
        .await
        .expect_err("customerless subtree reclassification");
    assert!(matches!(
        parent_error,
        AdminPortError::InvalidInput(message) if message == FINISHED_GOODS_CUSTOMER_REQUIRED
    ));
    let candidate_parent: String = sqlx::query_scalar(
        "SELECT COALESCE(parent_item_group, '')
         FROM mini_item_groups
         WHERE name = 'Candidate'",
    )
    .fetch_one(&pool)
    .await
    .expect("candidate parent after rollback");
    assert_eq!(candidate_parent, "All Item Groups");
    let upsert_error = store
        .create_item_group("Candidate", "Tayyor mahsulot", true)
        .await
        .expect_err("group upsert must not bypass parent protection");
    assert!(matches!(
        upsert_error,
        AdminPortError::InvalidInput(message) if message == FINISHED_GOODS_CUSTOMER_REQUIRED
    ));
    let candidate_parent: String = sqlx::query_scalar(
        "SELECT COALESCE(parent_item_group, '')
         FROM mini_item_groups
         WHERE name = 'Candidate'",
    )
    .fetch_one(&pool)
    .await
    .expect("candidate parent after rejected upsert");
    assert_eq!(candidate_parent, "All Item Groups");
    for table in [
        "mini_gscale_receipts",
        "mini_raw_material_stock",
        "mini_raw_material_assignments",
        "mini_finished_goods_stock",
        "mini_qolip_locations",
        "mini_qolip_product_specs",
        "mini_qolip_checkouts",
        "mini_quick_order_templates",
        "mini_rps_batches",
    ] {
        let (item_code, payload_code): (String, String) = sqlx::query_as(&format!(
            "SELECT item_code, payload_json->>'item_code' FROM {table} LIMIT 1"
        ))
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("{table} item identity: {error}"));
        assert_eq!(item_code, "ITEM-NEW", "{table} relational item code");
        assert_eq!(payload_code, "ITEM-NEW", "{table} payload item code");
    }
    for table in [
        "mini_gscale_receipts",
        "mini_raw_material_stock",
        "mini_raw_material_assignments",
        "mini_finished_goods_stock",
        "mini_qolip_locations",
        "mini_qolip_product_specs",
        "mini_qolip_checkouts",
    ] {
        let payload_name: String = sqlx::query_scalar(&format!(
            "SELECT payload_json->>'item_name' FROM {table} LIMIT 1"
        ))
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("{table} item name: {error}"));
        assert_eq!(payload_name, "New item name", "{table} payload item name");
    }

    let delete_error = store
        .delete_item("ITEM-NEW")
        .await
        .expect_err("active order must block item delete");
    assert!(matches!(
        delete_error,
        AdminPortError::InvalidInput(message) if message == "item is used by active order"
    ));
    assert!(store.item_detail("ITEM-NEW").await.is_ok());

    store
        .create_item("ITEM-UNUSED", "Unused item", "Kg", "All Item Groups")
        .await
        .expect("unused item");
    store
        .delete_item("ITEM-UNUSED")
        .await
        .expect("delete unused item");
    assert!(matches!(
        store.item_detail("ITEM-UNUSED").await,
        Err(AdminPortError::NotFound)
    ));

    pool.close().await;
    sqlx::query(&format!(r#"DROP DATABASE "{db_name}" WITH (FORCE)"#))
        .execute(&admin_pool)
        .await
        .expect("drop test db");
    admin_pool.close().await;
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_admin_item_safety"]
async fn postgres_admin_item_create_and_delete_safety_guards() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".to_string());
    let db_name = "mini_rs_erp_test_admin_item_safety";
    let admin_options = PgConnectOptions::from_str(&admin_url).expect("admin database url");
    let admin_pool = PgPoolOptions::new()
        .connect_with(admin_options.clone())
        .await
        .expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop stale test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");

    let pool = PgPoolOptions::new()
        .connect_with(admin_options.database(db_name))
        .await
        .expect("test db");
    apply_postgres_migrations_through(&pool, 18)
        .await
        .expect("apply item migrations");
    let store = PostgresAdminCatalogStore::new(pool.clone());

    store
        .create_item("ITEM-SAFE", "Safe item", "Kg", "All Item Groups")
        .await
        .expect("create item");
    let duplicate = store
        .create_item("item-safe", "Must not overwrite", "Dona", "All Item Groups")
        .await
        .expect_err("duplicate create");
    assert!(matches!(
        duplicate,
        AdminPortError::InvalidInput(message) if message == "item code already exists"
    ));
    let unchanged = store.item_detail("ITEM-SAFE").await.expect("unchanged");
    assert_eq!(unchanged.name, "Safe item");
    assert_eq!(unchanged.uom, "Kg");

    let race_store_a = store.clone();
    let race_store_b = store.clone();
    let (race_a, race_b) = tokio::join!(
        race_store_a.create_item("ITEM-RACE", "Race A", "Kg", "All Item Groups"),
        race_store_b.create_item("item-race", "Race B", "Dona", "All Item Groups"),
    );
    assert_ne!(race_a.is_ok(), race_b.is_ok());
    let race_winner_code = if race_a.is_ok() {
        "ITEM-RACE"
    } else {
        "item-race"
    };
    let race_error = if let Err(error) = race_a {
        error
    } else {
        race_b.expect_err("one concurrent create must fail")
    };
    assert!(matches!(
        race_error,
        AdminPortError::InvalidInput(message) if message == "item code already exists"
    ));
    let race_item = store
        .item_detail(race_winner_code)
        .await
        .expect("race winner");
    assert!(
        (race_item.name == "Race A" && race_item.uom == "Kg")
            || (race_item.name == "Race B" && race_item.uom == "Dona")
    );

    sqlx::query(
        "INSERT INTO mini_orders
             (id, code, order_number, product_code, product_name, status, kg)
         VALUES ('ORDER-SAFE', 'ORDER-SAFE', '9001', 'ITEM-SAFE', 'Safe item', 'draft', 1)",
    )
    .execute(&pool)
    .await
    .expect("active order");
    let active_order = store
        .delete_item("ITEM-SAFE")
        .await
        .expect_err("active order blocker");
    assert!(matches!(
        active_order,
        AdminPortError::InvalidInput(message) if message == "item is used by active order"
    ));
    assert!(store.item_detail("ITEM-SAFE").await.is_ok());

    sqlx::query("UPDATE mini_orders SET status = 'completed' WHERE id = 'ORDER-SAFE'")
        .execute(&pool)
        .await
        .expect("complete order");
    store
        .delete_item("ITEM-SAFE")
        .await
        .expect("terminal order allows delete");

    store
        .create_item(
            "ITEM-LEGACY",
            "Legacy name-only item",
            "Kg",
            "All Item Groups",
        )
        .await
        .expect("legacy item");
    sqlx::query(
        "INSERT INTO mini_orders
             (id, code, order_number, product_code, product_name, status, kg)
         VALUES ('ORDER-LEGACY', 'ORDER-LEGACY', '9002', '',
                 'Legacy name-only item', 'ready', 1)",
    )
    .execute(&pool)
    .await
    .expect("legacy name-only order");
    let legacy_order = store
        .delete_item("ITEM-LEGACY")
        .await
        .expect_err("legacy name fallback blocker");
    assert!(matches!(
        legacy_order,
        AdminPortError::InvalidInput(message) if message == "item is used by active order"
    ));
    assert!(store.item_detail("ITEM-LEGACY").await.is_ok());
    sqlx::query("UPDATE mini_orders SET status = 'cancelled' WHERE id = 'ORDER-LEGACY'")
        .execute(&pool)
        .await
        .expect("cancel legacy order");
    store
        .delete_item("ITEM-LEGACY")
        .await
        .expect("terminal legacy order allows delete");

    store
        .create_item("ITEM-STOCK", "Stock item", "Kg", "All Item Groups")
        .await
        .expect("stock item");
    sqlx::query(
        "INSERT INTO mini_raw_material_stock
             (id, warehouse, item_code, item_name, barcode, qty, uom, status, payload_json)
         VALUES ('RAW-STOCK-SAFETY', 'Homashyo', 'ITEM-STOCK', 'Stock item',
                 'BARCODE-STOCK-SAFETY', 1, 'Kg', 'available', '{}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("active stock");
    let active_stock = store
        .delete_item("ITEM-STOCK")
        .await
        .expect_err("active stock blocker");
    assert!(matches!(
        active_stock,
        AdminPortError::InvalidInput(message) if message == "item has active stock"
    ));
    assert!(store.item_detail("ITEM-STOCK").await.is_ok());

    pool.close().await;
    sqlx::query(&format!(r#"DROP DATABASE "{db_name}" WITH (FORCE)"#))
        .execute(&admin_pool)
        .await
        .expect("drop test db");
    admin_pool.close().await;
}
