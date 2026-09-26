use super::*;
use crate::core::pending_orders::{PendingOrderStore, test_pending_order};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    postgres_test_database_options,
};
use crate::db::postgres_calculate_order::PostgresCalculateOrderStore;
use crate::db::postgres_production_map::{
    PostgresProductionMapStore, pending_orders::PostgresPendingOrderStore,
};

#[tokio::test]
async fn pending_orders_deny_workers_list_image_and_completion() {
    let state = test_state();
    for role in [
        PrincipalRole::Aparatchi,
        PrincipalRole::Qolipchi,
        PrincipalRole::Boyoqchi,
        PrincipalRole::TayyorlovMasteri,
        PrincipalRole::HomashyoRezkachi,
    ] {
        let token = session(&state, role).await;
        for url in [
            "/v1/mobile/admin/pending-orders",
            "/v1/mobile/admin/pending-orders/image?id=zakaz-9011",
        ] {
            let response = build_router(state.clone())
                .oneshot(request("GET", url, &token))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{role:?}: {url}");
        }
        let response = build_router(state.clone())
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps/with-order",
                &token,
                r#"{"pending_order_id":"zakaz-9011"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{role:?}");
    }
}

// Opt-in PostgreSQL test: creates only a fresh random database, never replaces an existing one.
#[tokio::test]
#[ignore = "requires local PostgreSQL; creates an isolated database"]
async fn pending_orders_postgres_atomic_completion_and_worker_isolation() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .try_init();
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    let db_name = format!("mini_pending_test_{:016x}", rand::random::<u64>());
    eprintln!("isolated pending-order database: {db_name}");
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&admin_pool)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .unwrap();
    // The alternative backfill needs the factory catalog before migration 0122.
    apply_postgres_migrations_through_version(&pool, "0121_print_preflight_order_status")
        .await
        .unwrap();
    let mut state = test_state();
    state.apparatus = crate::core::apparatus_standard::CanonicalApparatusService::new(Arc::new(
        crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository::new(
            pool.clone(),
        ),
    ));
    state.apparatus.bootstrap_factory_defaults().await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    state.production_maps = ProductionMapService::new(
        Arc::new(PostgresProductionMapStore::new(pool.clone())),
        Arc::new(CanonicalServiceApparatusResolver::new(
            state.apparatus.clone(),
        )),
    );
    state.calculate_orders = Arc::new(PostgresCalculateOrderStore::new(pool.clone()));
    let store = Arc::new(PostgresPendingOrderStore(pool.clone()));
    state.pending_orders = Some(store.clone());
    let (mut pending, image) = test_pending_order("9011");
    pending.template.status = "flexo".into();
    pending.template.edge_allowance_mm = 40.0;
    store.create(pending.clone(), image.clone()).await.unwrap();
    store.create(pending.clone(), image.clone()).await.unwrap();
    let reopened = PostgresPendingOrderStore(pool.clone());
    assert_eq!(reopened.list().await.unwrap().len(), 1);
    assert_eq!(reopened.image(&pending.id).await.unwrap().body, image.body);
    assert!(state.production_maps.maps().await.unwrap().is_empty());
    let before = state.production_maps.live_snapshot().await.unwrap();
    assert!(before.maps.is_empty());
    assert!(before.visible_order_ids.values().all(Vec::is_empty));
    let token = session(&state, PrincipalRole::Admin).await;
    let response = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/admin/pending-orders", &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await[0]["template"]["kg"], 500.0);

    let map: serde_json::Value = serde_json::from_str(&pechat_order_map_json(
        "wrong-id",
        "Replaced",
        "9999",
        "apparatus:default:bosma_8",
    ))
    .unwrap();
    let mut edited = pending.template.clone();
    edited.product = "Edited product".into();
    edited.kg = 600.0;
    edited.order_number = "9999".into();
    edited.frame_product_size_mm = 250.0;
    edited.frame_count = 3.0;
    edited.edge_allowance_mm = 55.0;
    edited.waste_percent = 7.0;
    edited.roll_count = Some(10);
    edited.color = "Qizil".into();
    let body = serde_json::json!({"pending_order_id":pending.id,"map":map,
        "template":edited}).to_string();
    // Force the LAST business-table write to fail. Maps/template/image must roll back too.
    sqlx::query("ALTER TABLE mini_order_products ADD CONSTRAINT pending_test_failure CHECK (color <> 'Qizil')")
        .execute(&pool).await.unwrap();
    let failed = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        ))
        .await
        .unwrap();
    assert_eq!(
        failed.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "{}",
        json_body(failed).await
    );
    for table in [
        "mini_production_maps",
        "mini_quick_order_templates",
        "mini_quick_order_images",
        "mini_orders",
        "mini_order_products",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "rollback: {table}");
    }
    assert!(store.get(&pending.id).await.unwrap().completion.is_none());
    sqlx::query("ALTER TABLE mini_order_products DROP CONSTRAINT pending_test_failure")
        .execute(&pool)
        .await
        .unwrap();

    let (first, retry) = tokio::join!(
        build_router(state.clone()).oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body
        )),
        build_router(state.clone()).oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body
        ))
    );
    let first = first.unwrap();
    let retry = retry.unwrap();
    assert_eq!(first.status(), StatusCode::OK, "{}", json_body(first).await);
    assert_eq!(retry.status(), StatusCode::OK, "{}", json_body(retry).await);
    let done = store.get(&pending.id).await.unwrap().completion.unwrap();
    assert_eq!(done.saved.map.id, "zakaz-9011");
    assert_eq!(done.saved.map.order_number, "9011");
    assert!(done.template.source_map_id.starts_with("template-"));
    assert_ne!(done.template.source_map_id, "template-zakaz-9011");
    assert_eq!(done.saved.map.order_kg, Some(600.0));
    assert!(done.saved.map.base_length.unwrap() > 0.0);
    assert_eq!(done.template.product, "Edited product");
    assert_eq!(done.template.kg, 600.0);
    assert_eq!(done.template.frame_product_size_mm, 250.0);
    assert_eq!(done.template.frame_count, 3.0);
    assert_eq!(done.template.edge_allowance_mm, 55.0);
    assert_eq!(done.template.width_mm, 805.0);
    assert_eq!(done.saved.map.width_mm, Some(805.0));
    assert_eq!(done.template.waste_percent, 7.0);
    assert_eq!(done.template.color, "Qizil");
    assert!(store.list().await.unwrap().is_empty());
    for (table, expected) in [
        ("mini_production_maps", 2),
        ("mini_quick_order_templates", 1),
        ("mini_quick_order_images", 1),
        ("mini_orders", 1),
        ("mini_order_products", 1),
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, expected, "idempotency: {table}");
    }
    let link: Option<String> =
        sqlx::query_scalar("SELECT order_id FROM mini_production_maps WHERE id='zakaz-9011'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(link.as_deref(), Some("zakaz-9011"));
    let code: String = sqlx::query_scalar("SELECT code FROM mini_orders WHERE id='zakaz-9011'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(code, "9011", "business order code is separate from the reusable template code");
    assert!(
        state
            .production_maps
            .live_snapshot()
            .await
            .unwrap()
            .maps
            .iter()
            .any(|m| m.map.id == pending.id)
    );
    let response = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/pending-orders/image?id=zakaz-9011",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/webp");
    assert_eq!(
        to_bytes(response.into_body(), 100).await.unwrap().as_ref(),
        image.body
    );
    // Emergency reset owns pending orders too, but preserves reusable templates.
    let (another, another_image) = test_pending_order("9012");
    store.create(another, another_image).await.unwrap();
    let reset = crate::db::postgres_order_reset::PostgresOrderResetStore::new(pool.clone())
        .reset_all_orders()
        .await
        .unwrap();
    assert_eq!(reset.pending_orders_deleted, 2);
    assert!(store.list().await.unwrap().is_empty());
    let (replacement, replacement_image) = test_pending_order("9011");
    assert_ne!(replacement.template.id, pending.template.id);
    store.create(replacement, replacement_image).await.unwrap();
    state.production_maps.notify_live();
    let after_reset = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &token,
            &body,
        ))
        .await
        .unwrap();
    assert_eq!(
        after_reset.status(),
        StatusCode::OK,
        "{}",
        json_body(after_reset).await
    );
    let templates: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_quick_order_templates")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(templates, 2, "reset must preserve previous templates");
    let replacement = store.get(&pending.id).await.unwrap().completion.unwrap();
    assert_ne!(replacement.template.source_map_id, done.template.source_map_id);
    assert!(state.production_maps.raw_map(&done.template.source_map_id).await.unwrap().is_some());
    assert!(state.production_maps.raw_map(&replacement.template.source_map_id).await.unwrap().is_some());
    drop(state);
    drop(store);
    drop(reopened);
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{db_name}\""))
        .execute(&admin_pool)
        .await
        .unwrap();
    admin_pool.close().await;
}
