use super::*;
use crate::core::production_map::CanonicalServiceApparatusResolver;
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;
use crate::db::postgres_production_map::{
    PostgresProductionMapStore, pending_orders::PostgresPendingOrderStore,
};
use crate::telegram::{
    TelegramService,
    models::TelegramUserAccount,
    order::{TelegramOrderDraft, TelegramOrderLayer, TelegramOrderStep},
};

#[tokio::test]
#[ignore = "requires local PostgreSQL; creates an isolated database"]
async fn automatic_telegram_intake_is_atomic_retry_safe_and_preserves_pending_failures() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .try_init();
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
    let name = format!("mini_auto_order_test_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE \"{name}\""))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &name))
        .await
        .unwrap();
    let test_pool = pool.clone();
    // Always close and remove this test-owned database, including on assertion failures.
    let result = tokio::spawn(async move { exercise_intake(test_pool).await }).await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{name}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn exercise_intake(pool: sqlx::PgPool) {
    apply_foundation_migration(&pool).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let apparatus = CanonicalApparatusService::new(Arc::new(
        PostgresCanonicalApparatusRepository::new(pool.clone()),
    ));
    apparatus.bootstrap_factory_defaults().await.unwrap();
    let maps = ProductionMapService::new(
        Arc::new(PostgresProductionMapStore::new(pool.clone())),
        Arc::new(CanonicalServiceApparatusResolver::new(apparatus.clone())),
    );
    let pending = Arc::new(PostgresPendingOrderStore(pool.clone()));
    let service = TelegramService::new(directory.path().join("telegram.json"))
        .with_pending_orders(Some(pending.clone()))
        .with_automatic_orders(
            apparatus,
            maps.clone(),
            Arc::new(crate::core::calculate_materials::MemoryCalculateMaterialStore::new()),
            Arc::new(crate::google_sheets::NoopOrderSheetSink),
        );
    let account: TelegramUserAccount = serde_json::from_value(serde_json::json!({
        "telegram_user_id":"123", "username":"test", "display_name":"Manager", "role":"sales_manager", "invite_token":"test", "joined_at_unix":1234
    })).unwrap();

    for (index, (form, method)) in [
        ("rulon", automatic::PrintMethod::Metal),
        ("rulon", automatic::PrintMethod::Flexo),
        ("paket", automatic::PrintMethod::Metal),
        ("paket", automatic::PrintMethod::Flexo),
    ]
    .into_iter()
    .enumerate()
    {
        let number = (9011 + index).to_string();
        let (_, image) = crate::core::pending_orders::test_pending_order(&number);
        let draft = TelegramOrderDraft {
            order_number: number.clone(),
            customer_ref: "CUST-1".into(),
            customer_name: "Mijoz".into(),
            product_code: format!("ITEM-{index}"),
            product_name: "Mahsulot".into(),
            status: form.into(),
            print_method: Some(method),
            cold_glue: Some(true),
            edge_allowance_mm: Some(15.0),
            layers: (0..2)
                .map(|_| TelegramOrderLayer {
                    material_id: "builtin-pet".into(),
                    material: "pet".into(),
                    micron: "12".into(),
                })
                .collect(),
            tiraj_kg: Some(500.0),
            frame_product_size_mm: Some(300.0),
            frame_count: Some(2.0),
            diameter_mm: Some(45.5),
            roll_count: Some(6),
            step: TelegramOrderStep::Attachment,
            ..Default::default()
        };
        service
            .save_order_draft("123", draft.clone())
            .await
            .unwrap();
        let restarted = TelegramService::new(directory.path().join("telegram.json"));
        assert_eq!(restarted.order_draft("123").await.unwrap().unwrap(), draft);

        if index == 0 {
            sqlx::query("ALTER TABLE mini_order_products ADD CONSTRAINT automatic_test_failure CHECK (false)").execute(&pool).await.unwrap();
            let failed = service
                .persist_pending_order(&account, &draft, image.clone())
                .await
                .unwrap();
            assert!(!failed.completed);
            assert!(!failed.reason.is_empty());
            assert_eq!(pending.list().await.unwrap().len(), 1);
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
            sqlx::query("ALTER TABLE mini_order_products DROP CONSTRAINT automatic_test_failure")
                .execute(&pool)
                .await
                .unwrap();
        }
        let (first, retry) = tokio::join!(
            service.persist_pending_order(&account, &draft, image.clone()),
            service.persist_pending_order(&account, &draft, image.clone())
        );
        for result in [first.unwrap(), retry.unwrap()] {
            assert!(result.completed, "{}", result.reason);
        }
        let order_id = format!("zakaz-{number}");
        let done = pending.get(&order_id).await.unwrap().completion.unwrap();
        assert_eq!(done.template.production_options, draft.production_options());
        assert_eq!(done.template.status, form);
        assert_eq!(done.saved.map.order_number, number);
        assert!(
            done.saved
                .map
                .edges
                .iter()
                .any(|e| e.from.starts_with("laminate_") && e.to.starts_with("cold_glue_"))
        );
        assert!(
            done.saved
                .map
                .edges
                .iter()
                .any(|e| e.from.starts_with("final_cut_") && e.to == "end")
        );
        assert!(maps.map(&order_id).await.unwrap().is_some());
        assert!(
            maps.map(&done.template.source_map_id)
                .await
                .unwrap()
                .is_some()
        );
        let body: Vec<u8> =
            sqlx::query_scalar("SELECT body FROM mini_quick_order_images WHERE image_id=$1")
                .bind(&image.image_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(body, image.body);
        assert!(pending.list().await.unwrap().is_empty());
    }
    for (table, expected) in [
        ("mini_production_maps", 8),
        ("mini_quick_order_templates", 4),
        ("mini_orders", 4),
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, expected, "retry duplicated {table}");
    }
    let mut invalid = service.order_draft("123").await.unwrap().unwrap();
    invalid.order_number = "9015".into();
    invalid.roll_count = Some(999);
    let (_, image) = crate::core::pending_orders::test_pending_order("9015");
    let result = service
        .persist_pending_order(&account, &invalid, image.clone())
        .await
        .unwrap();
    assert!(!result.completed);
    assert!(result.reason.contains("Bosma"));
    assert_eq!(pending.list().await.unwrap().len(), 1);
    assert_eq!(pending.image("zakaz-9015").await.unwrap().body, image.body);
    assert!(maps.map("zakaz-9015").await.unwrap().is_none());
}
