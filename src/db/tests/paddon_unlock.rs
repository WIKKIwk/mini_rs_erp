use super::*;
use crate::core::production_map::{PaddonReceipt, QueueActionActor};

#[tokio::test]
async fn paddon_unlock_checks_owner_setting_receipt_and_resets_print_successors() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let db = format!(
        "mini_rs_erp_test_paddon_unlock_{:016x}",
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
    let creator = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "creator".into(),
        display_name: "Creator".into(),
    };
    let owner = QueueActionActor {
        ref_: "lock-owner".into(),
        display_name: "Lock owner".into(),
        ..creator.clone()
    };
    let other = QueueActionActor {
        ref_: "other-worker".into(),
        ..creator.clone()
    };
    let manager = QueueActionActor {
        role: "admin".into(),
        ref_: "admin".into(),
        display_name: "Admin".into(),
    };
    let collision = QueueActionActor {
        role: "boyoqchi".into(),
        ..owner.clone()
    };
    let paddon = service.create_paddon("", "", &creator).await.unwrap();
    let apparatus = "apparatus:default:asset-010";
    let mut map = test_map("paddon-unlock-order", "9583", "PRODUCT-1");
    map.nodes[1].apparatus_id = apparatus.into();
    service.upsert_map(map).await.unwrap();
    let mut batch = wip_batch(apparatus);
    batch.order_id = "paddon-unlock-order".into();
    batch.batch_id = "unlock-roll".into();
    batch.qr_payload = "PROGRESS:unlock-roll".into();
    store.put_order_progress_batch(batch.clone()).await.unwrap();
    service
        .add_paddon_item(&paddon.code, &batch.batch_id, &creator)
        .await
        .unwrap();
    service
        .set_active_rezka_paddon(apparatus, &owner, &paddon.code)
        .await
        .unwrap();
    service
        .confirm_paddon_print(&paddon.code, &owner)
        .await
        .unwrap();
    assert!(
        service
            .can_unlock_paddon(&paddon.code, &owner)
            .await
            .unwrap()
    );
    assert!(
        service
            .can_unlock_paddon(&paddon.code, &manager)
            .await
            .unwrap()
    );
    for denied in [&creator, &other, &collision] {
        assert!(
            !service
                .can_unlock_paddon(&paddon.code, denied)
                .await
                .unwrap()
        );
        assert_eq!(
            service.unlock_paddon(&paddon.code, denied).await,
            Err(ProductionMapError::PaddonUnlockForbidden)
        );
    }
    service
        .confirm_paddon_print(&paddon.code, &other)
        .await
        .unwrap();
    assert!(
        !service
            .can_unlock_paddon(&paddon.code, &other)
            .await
            .unwrap(),
        "reprinting does not take lock ownership"
    );
    let next = service
        .create_active_paddon_successor(&paddon.code, apparatus, &owner)
        .await
        .unwrap();
    let before = service.paddon_snapshot(&paddon.code).await.unwrap();
    let unlocked = service.unlock_paddon(&paddon.code, &owner).await.unwrap();
    assert!(unlocked.locked_at_unix.is_none());
    assert_eq!(unlocked.id, paddon.id);
    assert_eq!(
        service
            .active_rezka_paddon(apparatus, &owner)
            .await
            .unwrap(),
        Some(next.code.clone())
    );
    let after = service.paddon_snapshot(&paddon.code).await.unwrap();
    assert_eq!(after.items, before.items);
    assert_eq!(after.paddon.total_net_kg, before.paddon.total_net_kg);
    assert!(after.can_manage_items);
    assert!(
        service
            .selectable_rezka_paddons(200)
            .await
            .unwrap()
            .iter()
            .any(|p| p.code == paddon.code)
    );
    assert_eq!(
        service.unlock_paddon(&paddon.code, &owner).await.unwrap(),
        unlocked
    );
    service
        .confirm_paddon_print(&paddon.code, &owner)
        .await
        .unwrap();
    let new_next = service
        .create_active_paddon_successor(&paddon.code, apparatus, &owner)
        .await
        .unwrap();
    assert_ne!(
        new_next.code, next.code,
        "reprinting an unlocked paddon starts a new confirmation cycle"
    );
    assert!(
        service.paddon_snapshot(&next.code).await.is_ok(),
        "old successor is preserved"
    );
    service
        .update_paddon_management_settings(true, &manager)
        .await
        .unwrap();
    assert!(
        service
            .can_unlock_paddon(&paddon.code, &other)
            .await
            .unwrap()
    );
    service.unlock_paddon(&paddon.code, &other).await.unwrap();
    service
        .confirm_paddon_print(&paddon.code, &owner)
        .await
        .unwrap();
    // A disable committed while an unlock waits must reject the foreign unlock.
    let mut disable = pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE mini_paddon_management_settings SET free_movement_enabled=false WHERE singleton",
    )
    .execute(&mut *disable)
    .await
    .unwrap();
    let task_service = service.clone();
    let task_code = paddon.code.clone();
    let task_actor = other.clone();
    let attempt =
        tokio::spawn(async move { task_service.unlock_paddon(&task_code, &task_actor).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%mini_paddon_management_settings%FOR SHARE%')").fetch_one(&pool).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("unlock is waiting for the setting row");
    disable.commit().await.unwrap();
    assert_eq!(
        attempt.await.unwrap(),
        Err(ProductionMapError::PaddonUnlockForbidden)
    );
    assert!(
        service
            .can_unlock_paddon(&paddon.code, &owner)
            .await
            .unwrap()
    );
    let (first, second) = tokio::join!(
        service.unlock_paddon(&paddon.code, &owner),
        service.unlock_paddon(&paddon.code, &owner)
    );
    assert!(first.is_ok() && second.is_ok());
    service
        .confirm_paddon_print(&paddon.code, &owner)
        .await
        .unwrap();
    service.unlock_paddon(&paddon.code, &manager).await.unwrap();
    service
        .confirm_paddon_print(&paddon.code, &owner)
        .await
        .unwrap();
    service
        .update_paddon_management_settings(true, &manager)
        .await
        .unwrap();
    let receipt = PaddonReceipt {
        paddon: service.paddon_snapshot(&paddon.code).await.unwrap().paddon,
        items: before.items,
        stocks: vec![],
        warehouse: "WH-1".into(),
        accepted_by_ref: "keeper".into(),
        accepted_by_display_name: "Keeper".into(),
        accepted_at_unix: 1,
    };
    sqlx::query("UPDATE mini_paddons SET receipt_json=$2 WHERE code=$1")
        .bind(&paddon.code)
        .bind(serde_json::to_value(receipt).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    for actor in [&owner, &other, &manager] {
        assert!(
            !service
                .can_unlock_paddon(&paddon.code, actor)
                .await
                .unwrap()
        );
        assert_eq!(
            service.unlock_paddon(&paddon.code, actor).await,
            Err(ProductionMapError::PaddonAlreadyReceived)
        );
    }
    assert_eq!(
        service.unlock_paddon("missing", &owner).await,
        Err(ProductionMapError::PaddonNotFound)
    );
    assert_eq!(
        service.unlock_paddon(" ", &owner).await,
        Err(ProductionMapError::PaddonInvalidInput)
    );
    runtime.close().await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
