use super::*;
use crate::core::qolip::{QolipError, QolipService, QolipStorePort};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    canonical_apparatus_service, postgres_test_database_options,
};
use crate::db::postgres_qolip::PostgresQolipStore;

#[tokio::test]
async fn qolip_exact_product_lookup_ignores_search_limit_and_preserves_access() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let database = format!("qolip_exact_lookup_{:032x}", rand::random::<u128>());
    eprintln!("test database: {database}");
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(r#"CREATE DATABASE "{database}""#))
        .execute(&admin_pool)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &database))
        .await
        .unwrap();
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    canonical_apparatus_service(pool.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    apply_foundation_migration(&pool).await.expect("migrations");
    sqlx::raw_sql(
        r#"
        INSERT INTO mini_item_groups (name) VALUES ('All Item Groups') ON CONFLICT DO NOTHING;
        INSERT INTO mini_item_groups (name, parent_item_group)
            VALUES ('Tayyor mahsulot', 'All Item Groups') ON CONFLICT DO NOTHING;
        INSERT INTO mini_items (code, name, item_group) VALUES
            ('Zizi-other', 'A competing product', 'Tayyor mahsulot'),
            ('Zizi', 'Zizi sponge', 'Tayyor mahsulot'),
            ('EMPTY', 'No molds', 'Tayyor mahsulot');
        INSERT INTO mini_warehouses (id, name, parent_warehouse) VALUES
            ('owner-a', 'Molds A', ''), ('owner-b', 'Molds B', '');
        INSERT INTO mini_warehouse_assignments
            (warehouse, warehouse_name, assignment_kind, principal_role, principal_ref) VALUES
            ('Molds A', 'Molds A', 'warehouse', 'qolipchi', 'owner-a'),
            ('Molds B', 'Molds B', 'warehouse', 'qolipchi', 'owner-b');
        INSERT INTO mini_qolip_product_specs
            (item_code, item_name, item_group, qolip_code, size, payload_json)
        SELECT 'Zizi-other', 'A competing product', 'Tayyor mahsulot',
            'OTHER-' || lpad(n::text, 3, '0'), 40, '{"warehouse":"Molds A"}'::jsonb
        FROM generate_series(1, 115) n;
        INSERT INTO mini_qolip_product_specs
            (item_code, item_name, item_group, qolip_code, size, payload_json)
        SELECT 'Zizi', 'Zizi sponge', 'Tayyor mahsulot', 'ZIZI-' || lpad(n::text, 3, '0'), 40,
            jsonb_build_object('warehouse', CASE WHEN n <= 6 THEN 'Molds A' ELSE 'Molds B' END)
        FROM generate_series(1, 12) n;
    "#,
    )
    .execute(&pool)
    .await
    .expect("fixtures");

    let runtime_pool = sqlx::postgres::PgPoolOptions::new()
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(postgres_test_database_options(&admin_url, &database))
        .await
        .unwrap();
    let store = Arc::new(PostgresQolipStore::new(runtime_pool.clone()));
    let limited = store.products("Zizi", 100, false, None).await.unwrap();
    assert_eq!(limited.len(), 100);
    assert!(limited.iter().all(|product| product.code != "Zizi"));
    let full = store.products("Zizi", 1000, false, None).await.unwrap();
    assert_eq!(full.len(), 127);
    assert!(full[115..].iter().all(|product| product.code == "Zizi"));
    for code in ["Zizi", "  ZIZI  "] {
        let product = store.product_by_code(code).await.unwrap().unwrap();
        assert_eq!(product.code, "Zizi");
        assert!(
            product.qolip_code.is_empty(),
            "identity lookup carries no mold inventory"
        );
    }
    for code in [
        "Ziz",
        "Zizi%",
        "Zizi' OR TRUE --",
        "A competing product",
        "",
    ] {
        assert!(
            store.product_by_code(code).await.unwrap().is_none(),
            "{code}"
        );
    }
    let service = QolipService::new(store.clone());
    assert_eq!(
        service
            .required_qolips_for_order(" zIzI ", "")
            .await
            .unwrap()
            .len(),
        12
    );
    assert_eq!(
        service
            .required_qolips_for_order("missing", "Zizi sponge")
            .await,
        Err(QolipError::ProductNotFound)
    );

    let mut state = test_state();
    state.qolip = service.clone();
    let admin = session(&state, PrincipalRole::Admin).await;
    let owner_a = session_for(&state, PrincipalRole::Qolipchi, "owner-a").await;
    let owner_b = session_for(&state, PrincipalRole::Qolipchi, "owner-b").await;
    let unassigned = session_for(&state, PrincipalRole::Qolipchi, "unassigned").await;
    let worker = session_for(&state, PrincipalRole::Aparatchi, "worker").await;
    let router = build_router(state);
    let path = "/v1/mobile/qolip/shared-sets?item_code=%20zIzI%20";
    for (token, count, warehouse) in [
        (&admin, 12, None),
        (&owner_a, 6, Some("Molds A")),
        (&owner_b, 6, Some("Molds B")),
        (&unassigned, 0, None),
    ] {
        let response = router
            .clone()
            .oneshot(request("GET", path, token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        let products = body["products"].as_array().unwrap();
        assert_eq!(products.len(), count);
        assert!(products.iter().all(|p| p["code"] == "Zizi"));
        if let Some(warehouse) = warehouse {
            assert!(products.iter().all(|p| p["warehouse"] == warehouse));
        }
    }
    for (token, status) in [
        (worker.as_str(), StatusCode::FORBIDDEN),
        ("", StatusCode::UNAUTHORIZED),
    ] {
        let response = router
            .clone()
            .oneshot(request("GET", path, token))
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    let response = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/qolip/shared-sets?item_code=missing",
            &admin,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let missing = json_body(response).await;
    assert_eq!(missing["error"], "product_not_found");
    assert_eq!(
        missing["message"],
        "Mahsulot topilmadi. Mahsulot kodini tekshiring."
    );
    let response = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/qolip/shared-sets?item_code=EMPTY",
            &admin,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        json_body(response).await["products"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // The target's own molds must also remain unbounded by the catalog page size.
    sqlx::raw_sql(
        r#"
        INSERT INTO mini_qolip_product_specs
            (item_code, item_name, item_group, qolip_code, size, payload_json)
        SELECT 'Zizi', 'Zizi sponge', 'Tayyor mahsulot', 'ZIZI-' || lpad(n::text, 3, '0'),
            40, '{"warehouse":"Molds A"}'::jsonb FROM generate_series(13, 150) n;
    "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    let response = router
        .clone()
        .oneshot(request("GET", path, &admin))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["products"]
            .as_array()
            .unwrap()
            .len(),
        150
    );

    // Shared-set composition and legacy stock still pass through the existing resolver.
    sqlx::raw_sql(
        r#"
        INSERT INTO mini_qolip_product_overrides
            (item_code, qolip_code, mode, created_by_role, created_by_ref, created_by_name) VALUES
            ('Zizi', 'ZIZI-001', 'exclude', 'admin', 'admin', 'Admin'),
            ('Zizi', 'OTHER-001', 'include', 'admin', 'admin', 'Admin');
        INSERT INTO mini_qolip_locations
            (id, block, warehouse, item_code, item_name, qolip_code, size, quantity,
             created_by_role, created_by_ref) VALUES
            ('legacy-location', 'A', 'Molds A', 'Zizi', 'Zizi sponge', 'LEGACY-LOCATION', 40, 1,
             'qolipchi', 'owner-a');
        INSERT INTO mini_qolip_checkouts
            (id, location_id, block, warehouse, item_code, item_name, qolip_code, size, quantity,
             issued_to_ref, issued_to_name, issued_by_role, issued_by_ref) VALUES
            ('legacy-checkout', 'old-location', 'B', 'Molds B', 'Zizi', 'Zizi sponge',
             'LEGACY-CHECKOUT', 40, 1, 'worker', 'Worker', 'qolipchi', 'owner-b');
    "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    let molds = service.required_qolips_for_order("Zizi", "").await.unwrap();
    assert_eq!(molds.len(), 152);
    assert!(!molds.iter().any(|s| s.qolip_code == "ZIZI-001"));
    for code in ["OTHER-001", "LEGACY-LOCATION", "LEGACY-CHECKOUT"] {
        assert!(molds.iter().any(|s| s.qolip_code == code), "{code}");
    }

    // The normal planner uses the exact-code index as the catalog grows.
    sqlx::raw_sql(
        r#"
        INSERT INTO mini_items (code, name, item_group)
        SELECT 'PERF-' || n, 'Unrelated product ' || n, 'Tayyor mahsulot'
        FROM generate_series(1, 5000) n;
        ANALYZE mini_items;
    "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    let plan: Vec<String> = sqlx::query_scalar(
        "EXPLAIN SELECT code, name, item_group FROM mini_items
         WHERE lower(btrim(code)) = lower($1) ORDER BY (code = $1) DESC, code LIMIT 1",
    )
    .bind("zIzI")
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        plan.join("\n").contains("idx_mini_items_normalized_code"),
        "{plan:?}"
    );

    runtime_pool.close().await;
    let response = router.oneshot(request("GET", path, &admin)).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let failure = json_body(response).await;
    assert_eq!(failure["error"], "qolip_store_failed");
    assert_eq!(
        failure["message"],
        "Qoliplarni yuklashda server xatosi. Qayta urinib ko‘ring."
    );
    assert_ne!(failure["message"], missing["message"]);
    pool.close().await;
    sqlx::query(&format!(r#"DROP DATABASE "{database}" WITH (FORCE)"#))
        .execute(&admin_pool)
        .await
        .unwrap();
    admin_pool.close().await;
}
