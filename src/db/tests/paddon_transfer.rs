use super::*;
use crate::core::production_map::{PaddonTransferInput, QueueActionActor};
use sqlx::PgPool;

fn input(source: &str, target: &str, ids: &[&str]) -> PaddonTransferInput {
    PaddonTransferInput {
        source_code: source.into(),
        target_code: target.into(),
        progress_batch_ids: ids.iter().map(|id| (*id).into()).collect(),
    }
}

async fn members(pool: &PgPool, code: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT i.progress_batch_id FROM mini_paddon_items i JOIN mini_paddons p ON p.id=i.paddon_id WHERE p.code=$1 AND i.removed_at IS NULL ORDER BY i.progress_batch_id")
        .bind(code).fetch_all(pool).await.unwrap()
}

#[tokio::test]
async fn paddon_transfer_requires_unlocked_visible_pallets_and_commits_atomically() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = PgPool::connect(&url).await.unwrap();
    let db = format!(
        "mini_rs_erp_test_paddon_transfer_{:016x}",
        rand::random::<u64>()
    );
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(postgres_test_database_options(&url, &db))
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
    let worker = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "transfer-worker".into(),
        display_name: "Rezka".into(),
    };
    let manager = QueueActionActor {
        role: "admin".into(),
        ref_: "admin".into(),
        display_name: "Admin".into(),
    };
    let outsider = QueueActionActor {
        ref_: "other-worker".into(),
        ..worker.clone()
    };
    let source = service.create_paddon("", "", &worker).await.unwrap();
    let target = service.create_paddon("", "", &worker).await.unwrap();
    let third = service.create_paddon("", "", &worker).await.unwrap();
    let hidden = service.create_paddon("", "", &outsider).await.unwrap();
    let station = "apparatus:default:asset-010";
    let mut map = test_map("transfer-order", "9589", "PRODUCT-1");
    map.nodes[1].apparatus_id = station.into();
    service.upsert_map(map).await.unwrap();
    for id in ["transfer-1", "transfer-2"] {
        let mut batch = wip_batch(station);
        batch.batch_id = id.into();
        batch.order_id = "transfer-order".into();
        batch.qr_payload = format!("PROGRESS:{id}");
        store.put_order_progress_batch(batch).await.unwrap();
    }
    service
        .add_paddon_items(
            &source.code,
            &["transfer-1".into(), "transfer-2".into()],
            &worker,
        )
        .await
        .unwrap();
    let original = store.progress_batch("transfer-1").await.unwrap().unwrap();

    // The ordinary worker workflow does not require the global free movement flag.
    let result = service
        .transfer_paddon_items(
            input(&source.code, &target.code, &[" transfer-1 ", "transfer-1"]),
            &worker,
        )
        .await
        .unwrap();
    assert_eq!(result.progress_batch_ids, ["transfer-1"]);
    assert_eq!(members(&pool, &source.code).await, ["transfer-2"]);
    assert_eq!(members(&pool, &target.code).await, ["transfer-1"]);
    assert_eq!(
        store.progress_batch("transfer-1").await.unwrap().unwrap(),
        original
    );
    assert_eq!(
        service
            .transfer_paddon_items(input(&source.code, &target.code, &["transfer-1"]), &worker)
            .await,
        Err(ProductionMapError::PaddonItemNotAssigned),
        "stale/replayed requests cannot pull from another source"
    );
    service
        .transfer_paddon_items(input(&target.code, &source.code, &["transfer-1"]), &worker)
        .await
        .unwrap();
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_paddon_items WHERE progress_batch_id='transfer-1' AND removed_by_ref='transfer-worker' AND removed_at IS NOT NULL").fetch_one(&pool).await.unwrap();
    assert_eq!(audit, 2);
    for bad in [
        input(&source.code, &source.code, &["transfer-1"]),
        input(&source.code, &target.code, &[]),
    ] {
        assert_eq!(
            service.transfer_paddon_items(bad, &worker).await,
            Err(ProductionMapError::PaddonInvalidInput)
        );
    }
    assert_eq!(
        service
            .transfer_paddon_items(input(&source.code, &hidden.code, &["transfer-1"]), &worker)
            .await,
        Err(ProductionMapError::PaddonNotFound)
    );
    assert_eq!(
        service
            .transfer_paddon_items(
                input(&source.code, &target.code, &["transfer-1", "missing"]),
                &worker
            )
            .await,
        Err(ProductionMapError::ProgressBatchNotFound)
    );

    // Fail the second INSERT after the first membership was already changed.
    sqlx::query(&format!("ALTER TABLE mini_paddon_items ADD CONSTRAINT test_transfer_failure CHECK (paddon_id <> '{}' OR progress_batch_id <> 'transfer-2')", target.id)).execute(&pool).await.unwrap();
    assert_eq!(
        service
            .transfer_paddon_items(
                input(&source.code, &target.code, &["transfer-1", "transfer-2"]),
                &worker
            )
            .await,
        Err(ProductionMapError::StoreFailed)
    );
    assert_eq!(
        members(&pool, &source.code).await,
        ["transfer-1", "transfer-2"]
    );
    assert!(members(&pool, &target.code).await.is_empty());
    sqlx::query("ALTER TABLE mini_paddon_items DROP CONSTRAINT test_transfer_failure")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE mini_progress_batches SET wip_status='in_use' WHERE batch_id='transfer-2'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        service
            .transfer_paddon_items(
                input(&source.code, &target.code, &["transfer-1", "transfer-2"]),
                &worker
            )
            .await,
        Err(ProductionMapError::ProgressBatchNotAccepted)
    );
    sqlx::query("UPDATE mini_progress_batches SET wip_status='waiting', payload_json=payload_json || '{\"received_warehouse\":\"WH\"}'::jsonb WHERE batch_id='transfer-2'").execute(&pool).await.unwrap();
    assert_eq!(
        service
            .transfer_paddon_items(
                input(&source.code, &target.code, &["transfer-1", "transfer-2"]),
                &worker
            )
            .await,
        Err(ProductionMapError::ProgressBatchNotAccepted)
    );
    sqlx::query("UPDATE mini_progress_batches SET payload_json=payload_json - 'received_warehouse' WHERE batch_id='transfer-2'").execute(&pool).await.unwrap();

    service
        .update_paddon_management_settings(true, &manager)
        .await
        .unwrap();
    for code in [&source.code, &target.code] {
        service.confirm_paddon_print(code, &worker).await.unwrap();
        assert_eq!(
            service
                .transfer_paddon_items(input(&source.code, &target.code, &["transfer-1"]), &worker)
                .await,
            Err(ProductionMapError::PaddonLocked)
        );
        assert!(
            !service
                .paddon_snapshot(code)
                .await
                .unwrap()
                .can_manage_items
        );
        // Legacy add/remove routes cannot bypass the new seal rule either.
        assert_eq!(
            service
                .add_paddon_item(&target.code, "transfer-1", &worker)
                .await,
            Err(ProductionMapError::PaddonLocked)
        );
        if code == &source.code {
            assert_eq!(
                service
                    .remove_paddon_item(code, "transfer-1", &worker)
                    .await,
                Err(ProductionMapError::PaddonLocked)
            );
        }
        service.unlock_paddon(code, &worker).await.unwrap();
        sqlx::query("UPDATE mini_paddons SET receipt_json='{}' WHERE code=$1")
            .bind(code)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            service
                .transfer_paddon_items(input(&source.code, &target.code, &["transfer-1"]), &worker)
                .await,
            Err(ProductionMapError::PaddonAlreadyReceived)
        );
        sqlx::query("UPDATE mini_paddons SET receipt_json=NULL WHERE code=$1")
            .bind(code)
            .execute(&pool)
            .await
            .unwrap();
    }
    service
        .update_paddon_management_settings(false, &manager)
        .await
        .unwrap();
    service
        .update_paddon_settings(None, Some(true), &manager)
        .await
        .unwrap();
    assert_eq!(
        service
            .transfer_paddon_items(input(&source.code, &hidden.code, &["transfer-1"]), &worker)
            .await,
        Err(ProductionMapError::PaddonTransferForbidden),
        "visibility alone must not grant movement of another worker's stock"
    );
    service
        .update_paddon_management_settings(true, &manager)
        .await
        .unwrap();
    service
        .transfer_paddon_items(input(&source.code, &hidden.code, &["transfer-1"]), &worker)
        .await
        .unwrap();
    service
        .transfer_paddon_items(input(&hidden.code, &source.code, &["transfer-1"]), &worker)
        .await
        .unwrap();

    let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            service
                .transfer_paddon_items(input(&source.code, &target.code, &["transfer-1"]), &worker),
            service
                .transfer_paddon_items(input(&source.code, &third.code, &["transfer-1"]), &worker),
        )
    })
    .await
    .expect("concurrent transfers must not deadlock");
    assert_ne!(
        first.is_ok(),
        second.is_ok(),
        "only one destination may win"
    );
    let winner = if first.is_ok() {
        &target.code
    } else {
        &third.code
    };
    service
        .transfer_paddon_items(input(winner, &source.code, &["transfer-1"]), &worker)
        .await
        .unwrap();

    // Print/receipt takes the same pallet lock. Eligibility is re-read after waiting.
    let mut print = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM mini_paddons WHERE code=$1 FOR UPDATE")
        .bind(&source.code)
        .execute(&mut *print)
        .await
        .unwrap();
    let cloned = service.clone();
    let request = input(&source.code, &target.code, &["transfer-1"]);
    let actor = worker.clone();
    let mut pending =
        tokio::spawn(async move { cloned.transfer_paddon_items(request, &actor).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut pending)
            .await
            .is_err()
    );
    sqlx::query("UPDATE mini_paddons SET locked_at=now() WHERE code=$1")
        .bind(&source.code)
        .execute(&mut *print)
        .await
        .unwrap();
    print.commit().await.unwrap();
    assert_eq!(
        pending.await.unwrap(),
        Err(ProductionMapError::PaddonLocked)
    );
    assert_eq!(
        members(&pool, &source.code).await,
        ["transfer-1", "transfer-2"]
    );
    assert!(members(&pool, &target.code).await.is_empty());
    assert_eq!(
        store.progress_batch("transfer-1").await.unwrap().unwrap(),
        original
    );

    runtime.close().await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
