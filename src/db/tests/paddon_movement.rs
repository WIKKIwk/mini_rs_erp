use super::*;
use crate::core::production_map::QueueActionActor;

#[tokio::test]
async fn paddon_free_movement_is_atomic_audited_and_preserves_receipts() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let db = format!(
        "mini_rs_erp_test_paddon_move_{:016x}",
        rand::random::<u64>()
    );
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&url, &db))
        .await
        .unwrap();
    crate::db::postgres::apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    seed_standard_canonical_apparatus(&pool).await;
    apply_foundation_migration(&pool).await.unwrap();
    let runtime = sqlx::postgres::PgPoolOptions::new()
        .after_connect(|conn, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp").execute(conn).await?;
                Ok(())
            })
        })
        .connect_with(postgres_test_database_options(&url, &db))
        .await
        .unwrap();
    let store = Arc::new(PostgresProductionMapStore::new(runtime.clone()));
    let service = ProductionMapService::new_for_test(store.clone());
    let owner = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "roll-owner".into(),
        display_name: "Owner".into(),
    };
    let worker = QueueActionActor {
        ref_: "shift-worker".into(),
        display_name: "Next shift".into(),
        ..owner.clone()
    };
    let manager = QueueActionActor {
        role: "admin".into(),
        ref_: "manager".into(),
        display_name: "Admin".into(),
    };
    assert!(
        !service
            .paddon_management_settings()
            .await
            .unwrap()
            .free_movement_enabled
    );
    let source = service.create_paddon("", "", &owner).await.unwrap();
    let target = service.create_paddon("", "", &worker).await.unwrap();
    let open = service.create_paddon("", "", &worker).await.unwrap();
    let mut map = test_map("paddon-move-order", "9582", "PRODUCT-1");
    map.nodes[1].apparatus_id = "apparatus:default:asset-010".into();
    service.upsert_map(map).await.unwrap();
    let mut ids = Vec::new();
    for i in 1..=2 {
        let mut batch = wip_batch("apparatus:default:asset-010");
        batch.order_id = "paddon-move-order".into();
        batch.batch_id = format!("movement-{i}");
        batch.qr_payload = format!("PROGRESS:movement-{i}");
        batch.worker_ref = owner.ref_.clone();
        store.put_order_progress_batch(batch.clone()).await.unwrap();
        ids.push(batch.batch_id);
    }
    service
        .add_paddon_items(&source.code, &ids, &owner)
        .await
        .unwrap();
    service
        .confirm_paddon_print(&source.code, &owner)
        .await
        .unwrap();
    service
        .confirm_paddon_print(&target.code, &worker)
        .await
        .unwrap();
    assert_eq!(
        service
            .add_paddon_item(&target.code, &ids[0], &worker)
            .await,
        Err(ProductionMapError::PaddonLocked)
    );
    assert_eq!(
        service.add_paddon_item(&open.code, &ids[0], &worker).await,
        Err(ProductionMapError::PaddonItemAlreadyAssigned)
    );
    assert!(
        service
            .update_paddon_management_settings(true, &worker)
            .await
            .is_err()
    );
    service
        .update_paddon_management_settings(true, &manager)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT updated_by_ref FROM mini_paddon_management_settings"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        "manager"
    );
    let detail = service.paddon_snapshot(&target.code).await.unwrap();
    assert!(detail.free_movement_enabled && detail.can_manage_items);
    assert!(
        ids.iter()
            .all(|id| detail.available_items.iter().any(|b| &b.batch_id == id))
    );
    let moved = service
        .add_paddon_items(&target.code, &ids, &worker)
        .await
        .unwrap();
    assert_eq!(moved.items.len(), 2);
    assert!(
        service
            .paddon_snapshot(&source.code)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        moved
            .items
            .iter()
            .all(|b| b.worker_ref == owner.ref_ && b.produced_qty == 100.0)
    );
    service
        .add_paddon_items(&target.code, &ids, &worker)
        .await
        .unwrap();
    let audit: (i64, i64) = sqlx::query_as("SELECT COUNT(*) FILTER (WHERE removed_by_ref='shift-worker'), COUNT(*) FILTER (WHERE added_by_ref='shift-worker' AND removed_at IS NULL) FROM mini_paddon_items WHERE progress_batch_id=ANY($1)")
        .bind(&ids).fetch_one(&pool).await.unwrap();
    assert_eq!(audit, (2, 2));
    assert_eq!(
        service
            .add_paddon_items(&open.code, &[ids[0].clone(), "zz-missing".into()], &worker)
            .await,
        Err(ProductionMapError::ProgressBatchNotFound)
    );
    assert_eq!(
        service
            .paddon_snapshot(&target.code)
            .await
            .unwrap()
            .items
            .len(),
        2,
        "bulk failure rolls back every move"
    );
    let (a, b) = tokio::join!(
        service.add_paddon_item(&open.code, &ids[0], &worker),
        service.add_paddon_item(&source.code, &ids[0], &worker),
    );
    assert!(a.is_ok() || b.is_ok());
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM mini_paddon_items WHERE progress_batch_id=$1 AND removed_at IS NULL").bind(&ids[0]).fetch_one(&pool).await.unwrap(), 1);
    service
        .remove_paddon_item(&target.code, &ids[1], &worker)
        .await
        .unwrap();
    service
        .add_paddon_item(&target.code, &ids[1], &worker)
        .await
        .unwrap();
    service
        .update_paddon_management_settings(false, &manager)
        .await
        .unwrap();
    assert!(
        !service
            .paddon_snapshot(&target.code)
            .await
            .unwrap()
            .can_manage_items
    );
    assert_eq!(
        service
            .remove_paddon_item(&target.code, &ids[1], &worker)
            .await,
        Err(ProductionMapError::PaddonLocked)
    );
    service
        .update_paddon_management_settings(true, &manager)
        .await
        .unwrap();
    sqlx::query("UPDATE mini_paddons SET receipt_json='{}' WHERE id=$1")
        .bind(&target.id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        service.add_paddon_item(&open.code, &ids[1], &worker).await,
        Err(ProductionMapError::PaddonAlreadyReceived)
    );
    assert_eq!(
        service
            .remove_paddon_item(&target.code, &ids[1], &worker)
            .await,
        Err(ProductionMapError::PaddonAlreadyReceived)
    );
    assert_eq!(
        service
            .add_paddon_item(&target.code, &ids[0], &worker)
            .await,
        Err(ProductionMapError::PaddonAlreadyReceived)
    );
    // A roll accepted separately also stays protected without a pallet receipt.
    let current_code = sqlx::query_scalar::<_, String>("SELECT p.code FROM mini_paddon_items i JOIN mini_paddons p ON p.id=i.paddon_id WHERE i.progress_batch_id=$1 AND i.removed_at IS NULL")
        .bind(&ids[0]).fetch_one(&pool).await.unwrap();
    sqlx::query("UPDATE mini_progress_batches SET payload_json=payload_json || '{\"received_warehouse\":\"Warehouse\"}'::jsonb WHERE batch_id=$1")
        .bind(&ids[0]).execute(&pool).await.unwrap();
    assert_eq!(
        service
            .remove_paddon_item(&current_code, &ids[0], &worker)
            .await,
        Err(ProductionMapError::ProgressBatchNotAccepted)
    );
    runtime.close().await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
