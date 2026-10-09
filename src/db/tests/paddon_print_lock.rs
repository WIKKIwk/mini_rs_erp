use std::sync::Arc;

use crate::core::production_map::{
    ProductionMapError, ProductionMapService, ProductionMapStorePort, QueueActionActor,
    QueueProgressInput, queue_state::ApparatusQueueAction as Action,
};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    postgres_test_database_options,
};
use crate::db::postgres_production_map::PostgresProductionMapStore;

#[tokio::test]
async fn printed_paddon_is_sealed_and_successor_receives_subsequent_outputs() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let db = format!("mini_rs_erp_test_print_lock_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&url, &db))
        .await
        .unwrap();
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    super::seed_standard_canonical_apparatus(&pool).await;
    apply_foundation_migration(&pool).await.unwrap();
    let runtime_pool = sqlx::postgres::PgPoolOptions::new()
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(postgres_test_database_options(&url, &db))
        .await
        .unwrap();
    let store = Arc::new(PostgresProductionMapStore::new(runtime_pool.clone()));
    let service = ProductionMapService::new_for_test(store.clone());
    let apparatus = "apparatus:default:asset-010";
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "print-worker".into(),
        display_name: "Rezka".into(),
    };
    let other = QueueActionActor {
        ref_: "second-worker".into(),
        ..actor.clone()
    };
    let old = service.create_paddon("", "", &actor).await.unwrap();
    service
        .set_active_rezka_paddon(apparatus, &actor, &old.code)
        .await
        .unwrap();
    service
        .set_active_rezka_paddon(apparatus, &other, &old.code)
        .await
        .unwrap();
    let order = "zakaz-print-lock-order";
    service.upsert_map(serde_json::from_value(serde_json::json!({
        "id":order,"product_code":order,"title":order,"order_number":"9581",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
          {"id":"cut","kind":"apparatus","title":"Rezka","apparatus_id":apparatus,"rezka_kadr_count":3},
          {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"end"}]
    })).unwrap()).await.unwrap();
    let start = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order,
            Action::Start,
            &[apparatus.into()],
            actor.clone(),
            QueueProgressInput::default(),
        )
        .await
        .unwrap();
    let cycle = start.session.unwrap().session_id;
    let input = |index| {
        QueueProgressInput {
        rezka_record_frame_index: Some(index), rezka_output_cycle: cycle.clone(),
        rezka_frames: vec![serde_json::from_value(serde_json::json!({"produced_qty":120.0,"gross_qty":12.0,"bobina_kg":0.5,"diameter":45.0})).unwrap()],
        ..Default::default()
    }
    };
    let mut first = service
        .prepare_apparatus_queue_action_with_progress(
            apparatus,
            order,
            Action::RollComplete,
            &[apparatus.into()],
            actor.clone(),
            input(1),
        )
        .await
        .unwrap();
    first.attach_active_paddon();
    let first = service
        .commit_prepared_queue_action(first)
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    assert_eq!(
        service
            .paddon_snapshot(&old.code)
            .await
            .unwrap()
            .items
            .len(),
        1
    );

    let confirmation = service
        .confirm_paddon_print(&old.code, &actor)
        .await
        .unwrap();
    assert!(confirmation.newly_locked);
    assert!(confirmation.paddon.locked_at_unix.is_some());
    assert_eq!(confirmation.apparatuses, vec![apparatus]);
    for worker in [&actor, &other] {
        assert_eq!(
            service
                .active_rezka_paddon(apparatus, worker)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            service
                .set_active_rezka_paddon(apparatus, worker, &old.code)
                .await,
            Err(ProductionMapError::PaddonLocked)
        );
    }
    assert!(
        !service
            .confirm_paddon_print(&old.code, &actor)
            .await
            .unwrap()
            .newly_locked
    );
    assert_eq!(
        service
            .add_paddon_item(&old.code, &first.batch_id, &actor)
            .await,
        Err(ProductionMapError::PaddonLocked)
    );
    assert_eq!(
        service
            .remove_paddon_item(&old.code, &first.batch_id, &actor)
            .await,
        Err(ProductionMapError::PaddonLocked)
    );
    assert!(
        service
            .selectable_rezka_paddons(200)
            .await
            .unwrap()
            .is_empty()
    );

    let mut unassigned = service
        .prepare_apparatus_queue_action_with_progress(
            apparatus,
            order,
            Action::RollComplete,
            &[apparatus.into()],
            actor.clone(),
            input(2),
        )
        .await
        .unwrap();
    unassigned.attach_active_paddon();
    let unassigned = service
        .commit_prepared_queue_action(unassigned)
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    assert!(!unassigned.qr_payload.is_empty());
    let memberships: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_paddon_items WHERE progress_batch_id=$1 AND removed_at IS NULL",
    )
    .bind(&unassigned.batch_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(memberships, 0, "WIP can be saved without an active pallet");
    assert_eq!(
        store.progress_batches_for_order(order).await.unwrap().len(),
        2
    );

    let (a, b) = tokio::join!(
        service.create_active_paddon_successor(&old.code, apparatus, &actor),
        service.create_active_paddon_successor(&old.code, apparatus, &actor)
    );
    let next = a.unwrap();
    assert_eq!(
        next.code,
        b.unwrap().code,
        "repeated confirmation creates one successor"
    );
    assert_eq!(
        service
            .active_rezka_paddon(apparatus, &actor)
            .await
            .unwrap(),
        Some(next.code.clone())
    );
    assert_eq!(
        service
            .active_rezka_paddon(apparatus, &other)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        service.selectable_rezka_paddons(200).await.unwrap().len(),
        1
    );
    let mut second = service
        .prepare_apparatus_queue_action_with_progress(
            apparatus,
            order,
            Action::RollComplete,
            &[apparatus.into()],
            actor.clone(),
            input(3),
        )
        .await
        .unwrap();
    second.attach_active_paddon();
    service.commit_prepared_queue_action(second).await.unwrap();
    assert_eq!(
        service
            .paddon_snapshot(&next.code)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(
        service
            .paddon_snapshot(&old.code)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    service
        .validate_paddon_receiving_items(
            &service.paddon_scan_snapshot(&old.code).await.unwrap().items,
        )
        .await
        .unwrap();
    runtime_pool.close().await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
