use std::sync::Arc;

use sqlx::postgres::PgConnectOptions;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::inventory_movements::{
    InventoryActor, InventoryAssetKind, InventoryAssetSelector, InventoryMovementError,
    InventoryMovementStorePort, InventoryTransferCreate, InventoryTransferStatus,
};
use crate::core::warehouses::{WarehouseDeleteRequest, WarehouseError, WarehouseService};
use crate::db::postgres::apply_foundation_migration;
use crate::db::postgres_inventory_movements::PostgresInventoryMovementStore;
use crate::db::postgres_warehouse::PostgresWarehouseStore;

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_warehouse_delete_race"]
async fn postgres_warehouse_delete_is_serialized_with_transfer_creation() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".to_string());
    let db_name = "mini_rs_erp_test_warehouse_delete_race";
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

    let test_options = admin_url
        .parse::<PgConnectOptions>()
        .expect("valid admin database url")
        .database(db_name);
    let pool = sqlx::PgPool::connect_with(test_options)
        .await
        .expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migrations");

    sqlx::raw_sql(
        r#"
        INSERT INTO mini_warehouses (id, name)
        VALUES
            ('warehouse-race-source', 'Race Source'),
            ('warehouse-race-destination', 'Race Destination');

        INSERT INTO mini_warehouse_assignments (
            warehouse, principal_role, principal_ref, display_name
        )
        VALUES
            ('Race Source', 'admin', 'ADMIN-RACE', 'Race Admin'),
            ('Race Destination', 'admin', 'ADMIN-RACE', 'Race Admin');

        INSERT INTO mini_raw_material_stock (
            id, warehouse, item_code, item_name, barcode,
            qty, uom, status, payload_json
        )
        VALUES (
            'raw:warehouse-race', 'Race Source', 'ITEM-RACE',
            'Race material', 'WAREHOUSE-RACE-1',
            1, 'kg', 'available', '{}'::jsonb
        );
        "#,
    )
    .execute(&pool)
    .await
    .expect("seed warehouse race");

    let actor = InventoryActor::new(
        Principal {
            role: PrincipalRole::Admin,
            display_name: "Race Admin".to_string(),
            legal_name: String::new(),
            ref_: "ADMIN-RACE".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        },
        true,
        ["Race Source".to_string()],
    );
    let movement_store = Arc::new(PostgresInventoryMovementStore::new(pool.clone()));
    let warehouse_service = Arc::new(WarehouseService::new(Arc::new(
        PostgresWarehouseStore::new(pool.clone()),
    )));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));

    let create_transfer = {
        let movement_store = movement_store.clone();
        let barrier = barrier.clone();
        async move {
            barrier.wait().await;
            movement_store
                .create_transfer(
                    &actor,
                    "transfer-warehouse-race",
                    &InventoryTransferCreate {
                        source_warehouse_id: "warehouse-race-source".to_string(),
                        destination_warehouse_id: "warehouse-race-destination".to_string(),
                        assets: vec![InventoryAssetSelector {
                            asset_kind: InventoryAssetKind::RawMaterial,
                            asset_ref: "raw:warehouse-race".to_string(),
                        }],
                        note: "warehouse delete race".to_string(),
                        idempotency_key: "warehouse-delete-race".to_string(),
                    },
                )
                .await
        }
    };
    let delete_warehouse = {
        let warehouse_service = warehouse_service.clone();
        let barrier = barrier.clone();
        async move {
            barrier.wait().await;
            warehouse_service
                .delete_warehouse(WarehouseDeleteRequest {
                    warehouse: "Race Source".to_string(),
                    delete_products: true,
                })
                .await
        }
    };

    let (transfer_result, delete_result) = tokio::join!(create_transfer, delete_warehouse);
    match (&transfer_result, &delete_result) {
        (Ok(transfer), Err(WarehouseError::HasActiveReservations(count))) => {
            assert_eq!(transfer.status, InventoryTransferStatus::Requested);
            assert!(*count >= 1);
        }
        (Err(InventoryMovementError::WarehouseNotFound), Ok(deleted)) => {
            assert_eq!(deleted.warehouse, "Race Source");
        }
        other => panic!("unexpected race outcome: {other:?}"),
    }

    let source_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_warehouses WHERE id = 'warehouse-race-source')",
    )
    .fetch_one(&pool)
    .await
    .expect("source warehouse state");
    let active_transfer_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mini_inventory_transfers
         WHERE id = 'transfer-warehouse-race'
           AND status IN ('requested', 'approved', 'in_transit')",
    )
    .fetch_one(&pool)
    .await
    .expect("active transfer state");
    assert!(
        source_exists || active_transfer_count == 0,
        "active transfer must not reference a deleted warehouse"
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
