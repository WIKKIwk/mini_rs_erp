use crate::core::apparatus_standard::CanonicalApparatusService;
use crate::core::apparatus_standard::test_support::TestApparatusSpec;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::inventory_movements::{
    InventoryActor, InventoryAsset, InventoryAssetKind, InventoryAssetSelector,
    InventoryAssetQuery, InventoryMovementService,
    InventoryMovementError, InventoryMovementStorePort, InventoryRelocationBatchCreate,
    InventoryRelocationCreate, InventoryReturnBatchCreate, InventoryTransferAction,
    InventoryTransferActionKind, InventoryTransferCreate, InventoryTransferQuery,
    InventoryTransferStatus,
};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    postgres_test_database_options,
};
use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;
use crate::db::postgres_inventory_movements::PostgresInventoryMovementStore;

use super::seed_canonical_apparatus;

#[tokio::test]
async fn postgres_preparation_inventory_resolves_order_label_without_apparatus_assignment() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = format!("mini_rs_erp_test_prep_inventory_{:016x}", rand::random::<u64>());
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#)).execute(&admin_pool).await.unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name)).await.unwrap();
    apply_postgres_migrations_through_version(&pool, "0121").await.unwrap();
    CanonicalApparatusService::new(std::sync::Arc::new(
        PostgresCanonicalApparatusRepository::new(pool.clone()),
    )).bootstrap_factory_defaults().await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    let apparatus: String = sqlx::query_scalar("SELECT id FROM mini_apparatus ORDER BY id LIMIT 1")
        .fetch_one(&pool).await.unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_warehouses(id,name) VALUES ('warehouse-prep','Tayyorlov ombori'),('warehouse-other','Other');
        INSERT INTO mini_items(code,name,item_group) VALUES ('PREP_INVENTORY_PE','PE PR','Rulon');
        INSERT INTO mini_production_maps(id,product_code,title,code,map_json,lifecycle_status,production_completed_at)
        VALUES ('zakaz-86','P','Avella 72 sht','0086',
            '{}',
            'production_completed',now());
        INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,uom,status)
        VALUES ('raw:prep','Tayyorlov ombori','PREP_INVENTORY_PE','PE PR','QR-PREP',10,'kg','available'),
               ('raw:other','Other','PREP_INVENTORY_PE','PE PR','QR-OTHER',10,'kg','available');
        INSERT INTO mini_gscale_receipts(name,status,item_code,warehouse,qty,barcode,payload_json)
        VALUES ('receipt-own','submitted','PREP_INVENTORY_PE','Other',10,'QR-FOREIGN',
                   '{"actor_role":"tayyorlov_masteri","actor_ref":"prep-1"}'),
               ('receipt-other','submitted','PREP_INVENTORY_PE','Other',10,'QR-FOREIGN-OTHER',
                   '{"actor_role":"tayyorlov_masteri","actor_ref":"prep-2"}');
        INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,uom,status,source_receipt_id)
        VALUES ('raw:foreign','Other','PREP_INVENTORY_PE','PE PR','QR-FOREIGN',10,'kg','available','receipt-own'),
               ('raw:foreign-other','Other','PREP_INVENTORY_PE','PE PR','QR-FOREIGN-OTHER',10,'kg','available','receipt-other');
    "#).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_assignments(barcode,order_id,apparatus,canonical_apparatus_id,item_code,item_group,payload_json) VALUES('QR-PREP','zakaz-86',$1,$1,'PREP_INVENTORY_PE','Rulon','{}')")
        .bind(&apparatus).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_assignments(barcode,order_id,apparatus,canonical_apparatus_id,item_code,item_group,payload_json) VALUES('QR-FOREIGN','zakaz-86',$1,$1,'PREP_INVENTORY_PE','Rulon','{}'),('QR-FOREIGN-OTHER','zakaz-86',$1,$1,'PREP_INVENTORY_PE','Rulon','{}')")
        .bind(&apparatus).execute(&pool).await.unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_inventory_transfers(id,idempotency_key,source_warehouse_id,source_warehouse,
            destination_warehouse_id,destination_warehouse,requested_by_role,requested_by_ref)
        VALUES ('prep-raw-transfer','raw-transfer','warehouse-prep','Tayyorlov ombori','warehouse-other','Other','admin','admin'),
               ('prep-fg-transfer','fg-transfer','warehouse-prep','Tayyorlov ombori','warehouse-other','Other','admin','admin');
        INSERT INTO mini_inventory_transfer_lines(transfer_id,asset_kind,asset_ref,qty,uom,source_physical_location_id)
        VALUES ('prep-raw-transfer','raw_material','raw:prep',10,'kg','warehouse-prep'),
               ('prep-fg-transfer','finished_goods','fg:one',10,'kg','warehouse-prep');
    "#).execute(&pool).await.unwrap();
    let actor = InventoryActor::new(Principal {
        role: PrincipalRole::TayyorlovMasteri, display_name: "Master".into(),
        legal_name: String::new(), ref_: "prep-1".into(), phone: String::new(), avatar_url: String::new(),
    }, false, ["Tayyorlov ombori".to_string()]);
    let runtime_pool = sqlx::postgres::PgPoolOptions::new()
        .after_connect(|connection, _| Box::pin(async move {
            sqlx::query("SET ROLE mini_rs_erp").execute(connection).await?;
            Ok(())
        }))
        .connect_with(postgres_test_database_options(&admin_url, &db_name)).await.unwrap();
    let service = InventoryMovementService::new(std::sync::Arc::new(PostgresInventoryMovementStore::new(runtime_pool.clone())));
    let assets = service.assets(&actor, InventoryAssetQuery { limit: 100, ..Default::default() }).await.unwrap();
    assert_eq!(assets.len(), 2);
    assert!(assets.iter().any(|asset| asset.identifier == "QR-PREP"));
    assert!(assets.iter().any(|asset| asset.identifier == "QR-FOREIGN"));
    let assignments = service.list_order_assignments(&actor, &["QR-PREP".into(), "QR-OTHER".into(), "QR-FOREIGN".into(), "QR-FOREIGN-OTHER".into()], &[]).await.unwrap();
    assert_eq!(assignments.len(), 2);
    assert!(assignments.iter().all(|assignment| assignment.order_label == "0086 · Avella 72 sht"));
    let unassigned_actor = InventoryActor::new(actor.principal.clone(), false, Vec::new());
    let foreign_assets = service.assets(&unassigned_actor, InventoryAssetQuery {
        warehouse_id: "warehouse-other".into(), limit: 100, ..Default::default()
    }).await.unwrap();
    assert_eq!(foreign_assets.len(), 1);
    assert_eq!(foreign_assets[0].identifier, "QR-FOREIGN");
    let foreign_assignment = service.list_order_assignments(&unassigned_actor, &["QR-FOREIGN".into(), "QR-FOREIGN-OTHER".into()], &[]).await.unwrap();
    assert_eq!(foreign_assignment.len(), 1);
    assert_eq!(foreign_assignment[0].order_label, "0086 · Avella 72 sht");
    let denied = service.create_transfer(&actor, InventoryTransferCreate {
        source_warehouse_id: "warehouse-other".into(), destination_warehouse_id: "warehouse-prep".into(),
        assets: vec![InventoryAssetSelector { asset_kind: InventoryAssetKind::RawMaterial, asset_ref: "raw:foreign".into() }],
        note: String::new(), idempotency_key: "prep-foreign-transfer".into(),
    }).await;
    assert!(matches!(denied, Err(InventoryMovementError::WarehouseForbidden)));
    let transfers = service.transfers(&actor, InventoryTransferQuery { limit: 100, ..Default::default() }).await.unwrap();
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].id, "prep-raw-transfer");
    let result = service.transfer_action(&actor, "prep-fg-transfer", InventoryTransferActionKind::Cancel,
        InventoryTransferAction { idempotency_key: "prep-no-fg".into(), note: String::new() }).await;
    assert!(matches!(result, Err(InventoryMovementError::WarehouseForbidden)));
    runtime_pool.close().await;
    pool.close().await;
    sqlx::query(&format!(r#"DROP DATABASE "{db_name}" WITH (FORCE)"#)).execute(&admin_pool).await.unwrap();
    admin_pool.close().await;
}

#[tokio::test]
async fn postgres_material_relocations_return_committed_assets_and_replay_safely() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = format!(
        "mini_rs_erp_test_inventory_relocation_{:016x}",
        rand::random::<u64>()
    );
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create isolated relocation db");
    let seed_pool =
        sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name))
            .await
            .expect("seed db");
    apply_postgres_migrations_through_version(&seed_pool, "0121")
        .await
        .expect("apply migrations before apparatus-dependent backfill");
    CanonicalApparatusService::new(std::sync::Arc::new(
        PostgresCanonicalApparatusRepository::new(seed_pool.clone()),
    ))
    .bootstrap_factory_defaults()
    .await
    .expect("bootstrap canonical factory apparatus");
    apply_foundation_migration(&seed_pool)
        .await
        .expect("apply all migrations");
    sqlx::raw_sql(
        r#"
        INSERT INTO mini_warehouses (id, name) VALUES
            ('warehouse-material', 'Ulug''bek''s'),
            ('warehouse-other', 'Other warehouse');
        INSERT INTO mini_factory_locations (id, name) VALUES
            ('state-print', 'Bosma oldi'),
            ('state-laminate', 'Laminatsiya oldi');
        INSERT INTO mini_raw_material_stock (
            id, warehouse, item_code, item_name, barcode, qty, uom, status
        ) VALUES
            ('raw:relocation-single', 'Ulug''bek''s', 'BOPP', 'BOPP',
                'RELOCATION-SINGLE', 100.000030, 'kg', 'available'),
            ('raw:relocation-batch', 'Ulug''bek''s', 'BOPP', 'BOPP',
                'RELOCATION-BATCH', 49.500001, 'kg', 'available'),
            ('raw:relocation-other', 'Other warehouse', 'BOPP', 'BOPP',
                'RELOCATION-OTHER', 22.000000, 'kg', 'available');
        "#,
    )
    .execute(&seed_pool)
    .await
    .expect("seed material warehouses and states");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .expect("restricted runtime pool");
    let store = PostgresInventoryMovementStore::new(pool.clone());
    let actor = InventoryActor::new(
        Principal {
            role: PrincipalRole::MaterialTaminotchi,
            display_name: "Material supplier".to_string(),
            legal_name: String::new(),
            ref_: "material-relocation".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        },
        false,
        ["ULUG'BEK'S".to_string()],
    );
    let assert_placement = |assets: &[InventoryAsset], location_id: &str| {
        for asset in assets {
            assert_eq!(asset.kind, InventoryAssetKind::RawMaterial);
            assert_eq!(asset.custody_warehouse_id, "warehouse-material");
            assert_eq!(asset.custody_warehouse, "Ulug'bek's");
            assert_eq!(asset.physical_location.id, location_id);
            assert_eq!(asset.status, "available");
            assert_eq!(asset.uom, "kg");
            assert_eq!(
                asset.qty,
                match asset.asset_ref.as_str() {
                    "raw:relocation-single" => 100.00003,
                    "raw:relocation-batch" => 49.500001,
                    _ => panic!("unexpected relocated asset"),
                }
            );
        }
    };

    let single = InventoryRelocationCreate {
        asset_kind: InventoryAssetKind::RawMaterial,
        asset_ref: "raw:relocation-single".to_string(),
        physical_location_id: "inventory_location:state:state-print".to_string(),
        idempotency_key: "material-relocation-single".to_string(),
        note: String::new(),
    };
    let moved = store
        .relocate(&actor, &single)
        .await
        .expect("single relocation returns the committed asset");
    assert_placement(std::slice::from_ref(&moved), &single.physical_location_id);
    let replay = store
        .relocate(&actor, &single)
        .await
        .expect("single replay");
    assert_placement(std::slice::from_ref(&replay), &single.physical_location_id);
    assert_eq!(replay.placement_version, moved.placement_version);

    let selectors = vec![
        InventoryAssetSelector {
            asset_kind: InventoryAssetKind::RawMaterial,
            asset_ref: single.asset_ref.clone(),
        },
        InventoryAssetSelector {
            asset_kind: InventoryAssetKind::RawMaterial,
            asset_ref: "raw:relocation-batch".to_string(),
        },
    ];
    let batch = InventoryRelocationBatchCreate {
        assets: selectors.clone(),
        physical_location_id: "inventory_location:state:state-laminate".to_string(),
        idempotency_key: "material-relocation-batch".to_string(),
        note: String::new(),
    };
    let moved_batch = store
        .relocate_batch(&actor, &batch)
        .await
        .expect("warehouse-to-state and state-to-state batch returns committed assets");
    assert_eq!(moved_batch.len(), 2);
    assert_placement(&moved_batch, &batch.physical_location_id);
    let replay_batch = store
        .relocate_batch(&actor, &batch)
        .await
        .expect("batch replay");
    assert_eq!(replay_batch.len(), 2);
    assert_placement(&replay_batch, &batch.physical_location_id);
    for (replay, moved) in replay_batch.iter().zip(&moved_batch) {
        assert_eq!(replay.placement_version, moved.placement_version);
    }

    let returns = InventoryReturnBatchCreate {
        assets: selectors,
        idempotency_key: "material-relocation-return".to_string(),
        note: String::new(),
    };
    let returned = store
        .return_to_warehouses_batch(&actor, &returns)
        .await
        .expect("state-to-warehouse return returns committed assets");
    assert_eq!(returned.len(), 2);
    let warehouse_location = "inventory_location:warehouse:warehouse-material";
    assert_placement(&returned, warehouse_location);
    let replay_return = store
        .return_to_warehouses_batch(&actor, &returns)
        .await
        .expect("return replay");
    assert_eq!(replay_return.len(), 2);
    assert_placement(&replay_return, warehouse_location);
    for (replay, returned) in replay_return.iter().zip(&returned) {
        assert_eq!(replay.placement_version, returned.placement_version);
    }

    let forbidden = InventoryRelocationCreate {
        asset_ref: "raw:relocation-other".to_string(),
        idempotency_key: "material-relocation-forbidden".to_string(),
        ..single.clone()
    };
    assert!(matches!(
        store.relocate(&actor, &forbidden).await,
        Err(InventoryMovementError::WarehouseForbidden)
    ));
    let cross_warehouse = InventoryRelocationCreate {
        physical_location_id: "inventory_location:warehouse:warehouse-other".to_string(),
        idempotency_key: "material-relocation-cross-warehouse".to_string(),
        ..single
    };
    assert!(matches!(
        store.relocate(&actor, &cross_warehouse).await,
        Err(InventoryMovementError::CrossWarehouseRelocation)
    ));
    let events: Vec<(String, i64)> = sqlx::query_as(
        "SELECT event_type, count(*) FROM mini_inventory_movement_events
         GROUP BY event_type ORDER BY event_type",
    )
    .fetch_all(&pool)
    .await
    .expect("relocation ledger");
    assert_eq!(
        events,
        vec![
            ("relocated".to_string(), 3),
            ("returned_to_warehouse".to_string(), 2)
        ]
    );
    let stock: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT warehouse, status, qty::text, COALESCE(placement.physical_location_id, '')
         FROM mini_raw_material_stock stock
         LEFT JOIN mini_inventory_placements placement
           ON placement.asset_kind = 'raw_material' AND placement.asset_ref = stock.id
         ORDER BY stock.id",
    )
    .fetch_all(&pool)
    .await
    .expect("persisted stock after relocations and rejected moves");
    assert_eq!(
        stock,
        vec![
            (
                "Ulug'bek's".to_string(),
                "available".to_string(),
                "49.500001".to_string(),
                warehouse_location.to_string()
            ),
            (
                "Other warehouse".to_string(),
                "available".to_string(),
                "22.000000".to_string(),
                String::new()
            ),
            (
                "Ulug'bek's".to_string(),
                "available".to_string(),
                "100.000030".to_string(),
                warehouse_location.to_string()
            ),
        ]
    );

    pool.close().await;
    seed_pool.close().await;
    sqlx::query(&format!(r#"DROP DATABASE "{db_name}" WITH (FORCE)"#))
        .execute(&admin_pool)
        .await
        .expect("cleanup isolated relocation db");
    admin_pool.close().await;
}

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
