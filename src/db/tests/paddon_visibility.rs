use super::*;
use crate::core::production_map::QueueActionActor;

#[tokio::test]
async fn postgres_paddon_visibility_filters_before_limit_and_persists_independent_settings() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin_pool = sqlx::PgPool::connect(&url).await.unwrap();
    let db = format!(
        "mini_rs_erp_test_paddon_visibility_{:016x}",
        rand::random::<u64>()
    );
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&admin_pool)
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
    let service = ProductionMapService::new_for_test(Arc::new(PostgresProductionMapStore::new(
        runtime.clone(),
    )));
    let worker = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "visibility-owner".into(),
        display_name: "Worker".into(),
    };
    let admin = QueueActionActor {
        role: "admin".into(),
        ref_: "visibility-admin".into(),
        display_name: "Admin".into(),
    };
    assert!(
        !service
            .paddon_management_settings()
            .await
            .unwrap()
            .worker_visibility_enabled
    );
    let own = service.create_paddon("", "", &worker).await.unwrap();
    let locked = service.create_paddon("", "", &worker).await.unwrap();
    service
        .confirm_paddon_print(&locked.code, &worker)
        .await
        .unwrap();
    sqlx::query("INSERT INTO mini_paddons(id,code,created_by_ref,created_by_display_name,updated_at) SELECT 'visibility-other-'||n, (80000+n)::text, 'other-worker', 'Worker', now()+interval '1 hour' FROM generate_series(1,205) n")
        .execute(&pool).await.unwrap();
    let own_page = service.visible_paddons(200, false, &worker).await.unwrap();
    assert_eq!(own_page.len(), 2);
    assert!(own_page.iter().all(|p| p.created_by_ref == worker.ref_));
    assert_eq!(
        service.visible_paddons(1, true, &worker).await.unwrap(),
        vec![own.clone()]
    );
    assert_eq!(
        service
            .visible_paddons(200, false, &admin)
            .await
            .unwrap()
            .len(),
        200
    );
    assert_eq!(
        service
            .update_paddon_settings(None, Some(true), &worker)
            .await,
        Err(ProductionMapError::PaddonInvalidInput)
    );
    service
        .update_paddon_settings(None, Some(true), &admin)
        .await
        .unwrap();
    let shared = service.visible_paddons(1, true, &worker).await.unwrap();
    assert_eq!(shared[0].created_by_ref, "other-worker");
    let apparatus = "apparatus:default:asset-010";
    service
        .set_active_rezka_paddon(apparatus, &worker, &shared[0].code)
        .await
        .unwrap();
    let saved = service
        .update_paddon_management_settings(true, &admin)
        .await
        .unwrap();
    assert!(saved.worker_visibility_enabled);
    assert!(saved.free_movement_enabled);
    let saved = service
        .update_paddon_settings(None, Some(false), &admin)
        .await
        .unwrap();
    assert!(!saved.worker_visibility_enabled);
    assert!(saved.free_movement_enabled);
    assert_eq!(
        service
            .active_rezka_paddon(apparatus, &worker)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        service
            .set_active_rezka_paddon(apparatus, &worker, &shared[0].code)
            .await,
        Err(ProductionMapError::PaddonNotFound)
    );
    let fresh_service = ProductionMapService::new_for_test(Arc::new(
        PostgresProductionMapStore::new(runtime.clone()),
    ));
    assert_eq!(
        fresh_service.paddon_management_settings().await.unwrap(),
        saved
    );
    assert_eq!(
        fresh_service
            .visible_paddons(1, true, &worker)
            .await
            .unwrap(),
        vec![own]
    );
    let audit: String = sqlx::query_scalar(
        "SELECT updated_by_ref FROM mini_paddon_management_settings WHERE singleton",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit, admin.ref_);
    runtime.close().await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}"))
        .execute(&admin_pool)
        .await
        .unwrap();
    admin_pool.close().await;
}
