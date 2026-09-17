use crate::core::apparatus_standard::test_support::TestApparatusSpec;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::inventory_movements::{
    InventoryActor, InventoryAssetKind, InventoryAssetSelector, InventoryMovementError,
    InventoryMovementStorePort, InventoryTransferCreate, InventoryTransferStatus,
};
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_inventory_movements::PostgresInventoryMovementStore;

use super::seed_canonical_apparatus;

#[tokio::test]
async fn postgres_inventory_transfer_preserves_six_decimal_quantity_end_to_end() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_inventory_precision";
    assert!(db_name.starts_with("mini_rs_erp_test_"));

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
        .expect("apply all migrations");
    apply_foundation_migration(&pool)
        .await
        .expect("migrations remain idempotent");

    let quantity_columns: Vec<(String, String, Option<i32>, Option<i32>)> = sqlx::query_as(
        r#"
        SELECT table_name, data_type, numeric_precision, numeric_scale
        FROM information_schema.columns
        WHERE (table_name, column_name) IN (
            ('mini_raw_material_stock', 'qty'),
            ('mini_inventory_transfer_lines', 'qty'),
            ('mini_inventory_movement_events', 'qty')
        )
        ORDER BY table_name
        "#,
    )
    .fetch_all(&pool)
    .await
    .expect("quantity column types");
    assert_eq!(quantity_columns.len(), 3);
    assert!(
        quantity_columns
            .iter()
            .all(|(_, data_type, precision, scale)| {
                data_type == "numeric" && *precision == Some(18) && *scale == Some(6)
            })
    );

    seed_canonical_apparatus(
        &pool,
        TestApparatusSpec::laminate("apparatus:precision:press", "Precision Press"),
    )
    .await;

    sqlx::raw_sql(
        r#"
        INSERT INTO mini_warehouses (id, name)
        VALUES
            ('warehouse-source', 'Sklad Source'),
            ('warehouse-destination', 'Sklad Destination');

        INSERT INTO mini_warehouse_assignments (
            warehouse, assignment_kind, warehouse_name, apparatus_id,
            principal_role, principal_ref, display_name
        )
        VALUES
            ('Sklad Source', 'warehouse', 'Sklad Source', NULL,
                'admin', 'ADMIN-1', 'Admin'),
            ('Sklad Destination', 'warehouse', 'Sklad Destination', NULL,
                'admin', 'ADMIN-1', 'Admin'),
            ('Sklad Destination', 'apparatus', NULL, 'apparatus:precision:press',
                'admin', 'ADMIN-APPARATUS', 'Apparatus Admin');

        INSERT INTO mini_system_users (id, role, name, phone)
        VALUES ('prep-transfer', 'tayyorlov_masteri', 'Preparation Transfer', '901234590');
        INSERT INTO mini_item_groups (name, parent_item_group, is_group)
        VALUES ('Transfer Raw', 'All Item Groups', true);
        INSERT INTO mini_items (code, name, uom, item_group)
        VALUES ('PREP-TRANSFER', 'Preparation Transfer Material', 'kg', 'Transfer Raw');
        INSERT INTO mini_preparation_materials (item_code, owner_ref, name_key, warehouse_name)
        VALUES ('PREP-TRANSFER', 'prep-transfer', 'preparation transfer material', 'Sklad Source');
        INSERT INTO mini_preparation_material_warehouse_scopes (
            item_code, warehouse_id, scope_kind, created_by_role, created_by_ref
        ) VALUES ('PREP-TRANSFER', 'warehouse-source', 'exclusive',
                  'tayyorlov_masteri', 'prep-transfer');

        INSERT INTO mini_raw_material_stock (
            id, warehouse, item_code, item_name, barcode,
            qty, uom, status, payload_json
        )
        VALUES (
            'raw:precision-0001', 'Sklad Source', 'ITEM-PRECISION',
            'Precision material', 'PRECISION-0001',
            13.000030, 'kg', 'available', '{}'::jsonb
        ), (
            'raw:precision-prep-0001', 'Sklad Source', 'PREP-TRANSFER',
            'Preparation Transfer Material', 'PREP-TRANSFER-0001',
            2.000000, 'kg', 'available', '{}'::jsonb
        );
        "#,
    )
    .execute(&pool)
    .await
    .expect("seed inventory transfer");

    let actor = InventoryActor::new(
        Principal {
            role: PrincipalRole::Admin,
            display_name: "Admin".to_string(),
            legal_name: String::new(),
            ref_: "ADMIN-1".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        },
        true,
        ["Sklad Source".to_string(), "Sklad Destination".to_string()],
    );
    let store = PostgresInventoryMovementStore::new(pool.clone());
    let transfer = store
        .create_transfer(
            &actor,
            "transfer-precision-0001",
            &InventoryTransferCreate {
                source_warehouse_id: "warehouse-source".to_string(),
                destination_warehouse_id: "warehouse-destination".to_string(),
                assets: vec![InventoryAssetSelector {
                    asset_kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:precision-0001".to_string(),
                }],
                note: "precision regression".to_string(),
                idempotency_key: "transfer-precision-idempotency-0001".to_string(),
            },
        )
        .await
        .expect("complete internal transfer without quantity mismatch");

    assert_eq!(transfer.status, InventoryTransferStatus::Received);
    assert_eq!(transfer.lines.len(), 1);
    assert_eq!(transfer.lines[0].qty, 13.00003);

    let recipient_refs: Vec<String> = sqlx::query_scalar(
        "SELECT target_ref FROM mini_inventory_transfer_chat_outbox
         WHERE transfer_id = 'transfer-precision-0001' ORDER BY target_ref",
    )
    .fetch_all(&pool)
    .await
    .expect("transfer chat recipients");
    assert!(
        recipient_refs.is_empty(),
        "internal transfers must not enqueue warehouse chat notifications"
    );

    let (stock_warehouse, stock_status, stock_qty): (String, String, String) = sqlx::query_as(
        "SELECT warehouse, status, qty::text
         FROM mini_raw_material_stock WHERE id = 'raw:precision-0001'",
    )
    .fetch_one(&pool)
    .await
    .expect("transferred stock");
    assert_eq!(stock_warehouse, "Sklad Destination");
    assert_eq!(stock_status, "available");
    assert_eq!(stock_qty, "13.000030");

    let transfer_qty: String = sqlx::query_scalar(
        "SELECT qty::text FROM mini_inventory_transfer_lines
         WHERE transfer_id = 'transfer-precision-0001'",
    )
    .fetch_one(&pool)
    .await
    .expect("transfer quantity");
    assert_eq!(transfer_qty, "13.000030");

    let movement_quantities: Vec<String> = sqlx::query_scalar(
        "SELECT qty::text FROM mini_inventory_movement_events
         WHERE transfer_id = 'transfer-precision-0001' ORDER BY occurred_at, event_type",
    )
    .fetch_all(&pool)
    .await
    .expect("movement quantities");
    assert_eq!(movement_quantities.len(), 4);
    assert!(movement_quantities.iter().all(|qty| qty == "13.000030"));

    let raw_material_deltas: Vec<String> = sqlx::query_scalar(
        "SELECT qty_delta::text FROM mini_raw_material_events
         WHERE source_id = 'transfer-precision-0001' ORDER BY event_type",
    )
    .fetch_all(&pool)
    .await
    .expect("raw material transfer ledger");
    assert_eq!(raw_material_deltas, vec!["13.000030", "-13.000030"]);

    let blocked = store
        .create_transfer(
            &actor,
            "transfer-precision-prep-blocked",
            &InventoryTransferCreate {
                source_warehouse_id: "warehouse-source".to_string(),
                destination_warehouse_id: "warehouse-destination".to_string(),
                assets: vec![InventoryAssetSelector {
                    asset_kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:precision-prep-0001".to_string(),
                }],
                note: "preparation material must stay scoped".to_string(),
                idempotency_key: "transfer-precision-prep-blocked-key".to_string(),
            },
        )
        .await;
    assert!(matches!(
        blocked,
        Err(InventoryMovementError::MaterialWarehouseScopeMissing)
    ));
    assert_eq!(
        sqlx::query_as::<_, (String, String)>(
            "SELECT warehouse, status FROM mini_raw_material_stock
             WHERE id = 'raw:precision-prep-0001'",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        ("Sklad Source".to_string(), "available".to_string())
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
