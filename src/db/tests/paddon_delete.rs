use super::*;
use crate::core::production_map::QueueActionActor;

#[tokio::test]
async fn paddon_delete_only_unused_and_serializes_with_roll_assignment() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let db_name = format!(
        "mini_rs_erp_test_paddon_delete_{:08x}",
        rand::random::<u32>()
    );
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&url, &db_name))
        .await
        .unwrap();
    crate::db::postgres::apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    seed_standard_canonical_apparatus(&pool).await;
    apply_foundation_migration(&pool).await.unwrap();
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new_for_test(store.clone());
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "rezka-worker".into(),
        display_name: "Rezka".into(),
    };
    let apparatus = "apparatus:default:asset-010";

    let empty = service.create_paddon("", "", &actor).await.unwrap();
    service
        .set_active_rezka_paddon(apparatus, &actor, &empty.code)
        .await
        .unwrap();
    service
        .delete_paddon(&format!(" {} ", empty.code))
        .await
        .unwrap();
    assert!(store.paddon_summary(&empty.code).await.unwrap().is_none());
    assert_eq!(
        service
            .active_rezka_paddon(apparatus, &actor)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        service.delete_paddon(&empty.code).await,
        Err(ProductionMapError::PaddonNotFound)
    );
    assert_eq!(
        service.delete_paddon(" ").await,
        Err(ProductionMapError::PaddonInvalidInput)
    );

    let used = service.create_paddon("", "", &actor).await.unwrap();
    assert_ne!(used.code, empty.code, "deleted codes must not be reused");
    let mut map = test_map("delete-paddon-order", "98765", "PRODUCT-1");
    map.nodes[1].apparatus_id = apparatus.into();
    service.upsert_map(map).await.unwrap();
    let mut batch = wip_batch(apparatus);
    batch.order_id = "delete-paddon-order".into();
    batch.batch_id = "delete-paddon-roll".into();
    batch.qr_payload = "PROGRESS:delete-paddon-roll".into();
    store.put_order_progress_batch(batch.clone()).await.unwrap();
    service
        .add_paddon_item(&used.code, &batch.batch_id, &actor)
        .await
        .unwrap();
    assert_eq!(
        service.delete_paddon(&used.code).await,
        Err(ProductionMapError::PaddonDeleteLocked)
    );
    service
        .remove_paddon_item(&used.code, &batch.batch_id, &actor)
        .await
        .unwrap();
    assert_eq!(
        service.paddon_summary(&used.code).await.unwrap().item_count,
        0
    );
    assert_eq!(
        service.delete_paddon(&used.code).await,
        Err(ProductionMapError::PaddonDeleteLocked)
    );

    for update in [
        "receipt_json = '{}'::jsonb",
        "updated_at = created_at + interval '1 second'",
    ] {
        let paddon = service.create_paddon("", "", &actor).await.unwrap();
        sqlx::query(&format!("UPDATE mini_paddons SET {update} WHERE id = $1"))
            .bind(&paddon.id)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            service.delete_paddon(&paddon.code).await,
            Err(ProductionMapError::PaddonDeleteLocked)
        );
    }
    // Both current document links and legacy payload-only movements block deletion.
    for legacy in [false, true] {
        let paddon = service.create_paddon("", "", &actor).await.unwrap();
        sqlx::query("INSERT INTO mini_inventory_movement_events
            (id, idempotency_key, event_type, asset_kind, asset_ref, qty, uom, actor_role, actor_ref,
             source_document_type, source_document_id, payload_json)
            VALUES ($1, $1, 'paddon_received', 'finished_goods', 'stock', 1, 'kg', 'werka', 'keeper',
                    'paddon_receipt', $2, $3)")
            .bind(&paddon.id).bind(if legacy { "" } else { &paddon.id })
            .bind(if legacy { serde_json::json!({"paddon_code":paddon.code}) } else { serde_json::json!({}) })
            .execute(&pool).await.unwrap();
        assert_eq!(
            service.delete_paddon(&paddon.code).await,
            Err(ProductionMapError::PaddonDeleteLocked)
        );
    }

    // Hold the same lock used by a roll write. Deletion must wait, then see
    // the newly committed membership instead of cascading it away.
    let racing = service.create_paddon("", "", &actor).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM mini_paddons WHERE id=$1 FOR UPDATE")
        .bind(&racing.id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO mini_paddon_items(id, paddon_id, progress_batch_id) VALUES ('racing-item', $1, $2)")
        .bind(&racing.id).bind(&batch.batch_id).execute(&mut *tx).await.unwrap();
    let deleting_store = store.clone();
    let code = racing.code.clone();
    let mut deletion = tokio::spawn(async move { deleting_store.delete_paddon(&code).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut deletion)
            .await
            .is_err()
    );
    tx.commit().await.unwrap();
    assert_eq!(
        deletion.await.unwrap(),
        Err(ProductionMapError::PaddonDeleteLocked)
    );
    assert_eq!(
        service
            .paddon_summary(&racing.code)
            .await
            .unwrap()
            .item_count,
        1
    );

    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db_name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
