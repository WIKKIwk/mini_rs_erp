use std::sync::Arc;

use sqlx::postgres::PgConnectOptions;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::inventory_movements::{
    InventoryActor, InventoryAssetKind, InventoryAssetSelector, InventoryMovementError,
    InventoryMovementStorePort, InventoryTransferCreate, InventoryTransferStatus,
};
use crate::core::warehouses::{
    WarehouseAssignment, WarehouseAssignmentDeleteRequest, WarehouseDeleteRequest, WarehouseError,
    WarehouseService, WarehouseStorePort,
};
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
        INSERT INTO mini_apparatus (id, name, base_name, kind, payload_json)
        VALUES (
            'apparatus:test:one', 'Warehouse Race Apparatus',
            'Warehouse Race Apparatus', 'test', '{}'::jsonb
        );

        INSERT INTO mini_warehouses (id, name)
        VALUES
            ('warehouse-race-source', 'Race Source'),
            ('warehouse-race-destination', 'Race Destination');

        INSERT INTO mini_warehouse_assignments (
            assignment_kind, warehouse, warehouse_name, apparatus_id,
            principal_role, principal_ref, display_name
        )
        VALUES
            ('warehouse', 'Race Source', 'Race Source', NULL,
             'admin', 'ADMIN-RACE', 'Race Admin'),
            ('warehouse', 'Race Destination', 'Race Destination', NULL,
             'admin', 'ADMIN-RACE', 'Race Admin'),
            ('apparatus', 'apparatus:test:one', NULL, 'apparatus:test:one',
             'admin', 'ADMIN-RACE', 'Apparatus Assignment');

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

    let duplicate_warehouse_assignment = sqlx::query(
        "INSERT INTO mini_warehouse_assignments (
             assignment_kind, warehouse, warehouse_name, apparatus_id,
             principal_role, principal_ref, display_name
         )
         VALUES ('warehouse', 'legacy-race-source', 'Race Source', NULL,
                 'admin', 'ADMIN-RACE', 'Duplicate Warehouse Assignment')",
    )
    .execute(&pool)
    .await;
    assert!(
        duplicate_warehouse_assignment.is_err(),
        "canonical warehouse assignment must be unique per principal"
    );

    let duplicate_apparatus_assignment = sqlx::query(
        "INSERT INTO mini_warehouse_assignments (
             assignment_kind, warehouse, warehouse_name, apparatus_id,
             principal_role, principal_ref, display_name
         )
         VALUES ('apparatus', 'legacy-apparatus-snapshot', NULL,
                 'apparatus:test:one', 'admin', 'ADMIN-RACE',
                 'Duplicate Apparatus Assignment')",
    )
    .execute(&pool)
    .await;
    assert!(
        duplicate_apparatus_assignment.is_err(),
        "canonical apparatus assignment must be unique per principal"
    );

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
            assert_eq!(deleted.deleted_assignment_count, 1);
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
    let apparatus_assignment_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mini_warehouse_assignments
         WHERE assignment_kind = 'apparatus'
           AND apparatus_id = 'apparatus:test:one'",
    )
    .fetch_one(&pool)
    .await
    .expect("apparatus assignment state");
    assert_eq!(apparatus_assignment_count, 1);
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

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_warehouse_canonical_identity"]
async fn postgres_warehouse_assignments_use_typed_canonical_identity() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".to_string());
    let db_name = "mini_rs_erp_test_warehouse_canonical_identity";
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
        INSERT INTO mini_apparatus (id, name, base_name, kind, payload_json)
        VALUES (
            'apparatus:test:canonical', 'Warehouse Canonical Apparatus',
            'Warehouse Canonical Apparatus', 'test', '{}'::jsonb
        );

        INSERT INTO mini_warehouses (id, name)
        VALUES ('warehouse-canonical', 'Canonical Warehouse');
        "#,
    )
    .execute(&pool)
    .await
    .expect("seed canonical warehouse state");

    let store = PostgresWarehouseStore::new(pool.clone());
    let first = store
        .put_warehouse_assignment(WarehouseAssignment {
            assignment_kind: "apparatus".to_string(),
            warehouse: "apparatus display snapshot before".to_string(),
            warehouse_name: None,
            apparatus_id: Some("apparatus:test:canonical".to_string()),
            principal_role: PrincipalRole::Admin,
            principal_ref: "ADMIN-CANONICAL".to_string(),
            display_name: "Before rename".to_string(),
        })
        .await
        .expect("insert canonical apparatus assignment");
    assert_eq!(first.display_name, "Before rename");

    let updated = store
        .put_warehouse_assignment(WarehouseAssignment {
            assignment_kind: "apparatus".to_string(),
            warehouse: "apparatus display snapshot after".to_string(),
            warehouse_name: None,
            apparatus_id: Some("apparatus:test:canonical".to_string()),
            principal_role: PrincipalRole::Admin,
            principal_ref: "ADMIN-CANONICAL".to_string(),
            display_name: "After rename".to_string(),
        })
        .await
        .expect("update canonical apparatus assignment");
    assert_eq!(updated.warehouse, "apparatus display snapshot after");
    assert_eq!(updated.display_name, "After rename");

    let canonical_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mini_warehouse_assignments
         WHERE assignment_kind = 'apparatus'
           AND apparatus_id = 'apparatus:test:canonical'
           AND principal_role = 'admin'
           AND principal_ref = 'ADMIN-CANONICAL'",
    )
    .fetch_one(&pool)
    .await
    .expect("canonical assignment count");
    assert_eq!(canonical_count, 1);

    store
        .put_warehouse_assignment(WarehouseAssignment {
            assignment_kind: "warehouse".to_string(),
            warehouse: "Canonical Warehouse".to_string(),
            warehouse_name: Some("Canonical Warehouse".to_string()),
            apparatus_id: None,
            principal_role: PrincipalRole::Admin,
            principal_ref: "ADMIN-DISJOINT".to_string(),
            display_name: "Warehouse identity".to_string(),
        })
        .await
        .expect("insert warehouse identity");
    store
        .put_warehouse_assignment(WarehouseAssignment {
            assignment_kind: "apparatus".to_string(),
            warehouse: "Apparatus snapshot".to_string(),
            warehouse_name: None,
            apparatus_id: Some("apparatus:test:canonical".to_string()),
            principal_role: PrincipalRole::Admin,
            principal_ref: "ADMIN-DISJOINT".to_string(),
            display_name: "Apparatus identity".to_string(),
        })
        .await
        .expect("insert disjoint apparatus identity");

    let (warehouse_count, apparatus_count): (i64, i64) = sqlx::query_as(
        "SELECT
             COUNT(*) FILTER (WHERE assignment_kind = 'warehouse'),
             COUNT(*) FILTER (WHERE assignment_kind = 'apparatus')
         FROM mini_warehouse_assignments
         WHERE principal_role = 'admin'
           AND principal_ref = 'ADMIN-DISJOINT'",
    )
    .fetch_one(&pool)
    .await
    .expect("typed identity counts");
    assert_eq!((warehouse_count, apparatus_count), (1, 1));

    let service = WarehouseService::new(Arc::new(PostgresWarehouseStore::new(pool.clone())));
    let principal = Principal {
        role: PrincipalRole::Admin,
        display_name: "Disjoint Admin".to_string(),
        legal_name: String::new(),
        ref_: "ADMIN-DISJOINT".to_string(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    assert_eq!(store.warehouse_assignments("").await.unwrap().len(), 1);
    assert_eq!(
        service
            .warehouse_assignments_for_principal(&principal)
            .await
            .expect("read both typed assignments")
            .len(),
        2
    );

    let removed_apparatus = service
        .unassign_warehouse(WarehouseAssignmentDeleteRequest {
            assignment_kind: "apparatus".to_string(),
            warehouse: "Apparatus snapshot".to_string(),
            warehouse_name: None,
            apparatus_id: Some("apparatus:test:canonical".to_string()),
            principal_role: PrincipalRole::Admin,
            principal_ref: "ADMIN-DISJOINT".to_string(),
        })
        .await
        .expect("delete apparatus assignment by typed id");
    assert_eq!(removed_apparatus.assignment_kind, "apparatus");
    assert_eq!(store.warehouse_assignments("").await.unwrap().len(), 1);
    assert_eq!(
        service
            .warehouse_assignments_for_principal(&principal)
            .await
            .expect("read warehouse assignment after apparatus delete")
            .len(),
        1
    );

    let removed_warehouse = service
        .unassign_warehouse(WarehouseAssignmentDeleteRequest {
            assignment_kind: "warehouse".to_string(),
            warehouse: "Canonical Warehouse".to_string(),
            warehouse_name: Some("Canonical Warehouse".to_string()),
            apparatus_id: None,
            principal_role: PrincipalRole::Admin,
            principal_ref: "ADMIN-DISJOINT".to_string(),
        })
        .await
        .expect("delete warehouse assignment");
    assert_eq!(removed_warehouse.assignment_kind, "warehouse");
    assert!(
        service
            .warehouse_assignments_for_principal(&principal)
            .await
            .expect("read assignments after both deletes")
            .is_empty()
    );

    let orphan = sqlx::query(
        "INSERT INTO mini_warehouse_assignments (
             assignment_kind, warehouse, warehouse_name, apparatus_id,
             principal_role, principal_ref, display_name
         )
         VALUES ('apparatus', 'orphan display snapshot', NULL,
                 'apparatus:test:missing', 'admin', 'ADMIN-ORPHAN', 'Orphan')",
    )
    .execute(&pool)
    .await;
    assert!(orphan.is_err(), "orphan apparatus assignment must be rejected");

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
