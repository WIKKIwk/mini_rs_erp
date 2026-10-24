#![cfg(feature = "verification")]

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use mini_rs_erp::app::AppState;
use mini_rs_erp::config::AppConfig;
use mini_rs_erp::core::auth::models::{Principal, PrincipalRole};
use mini_rs_erp::core::qolip::{QolipService, QolipStorePort};
use mini_rs_erp::core::session::manager::SessionManager;
use mini_rs_erp::http::router::build_router;
use std::sync::Arc;
use tower::ServiceExt;

async fn verify_http_catalog(pool: &sqlx::PgPool) {
    use mini_rs_erp::db::postgres_qolip::PostgresQolipStore;
    let (mut state, _files) = test_state();
    state.qolip = QolipService::new(Arc::new(PostgresQolipStore::new(pool.clone())));
    let token = session_for(&state, PrincipalRole::Qolipchi, "owner-a").await;
    let denied = session_for(&state, PrincipalRole::Customer, "catalog-customer").await;
    let app = build_router(state);
    let base = "/v1/mobile/qolip/products/page?limit=200";
    let mut path = base.to_owned();
    let mut seen = std::collections::BTreeSet::new();
    let mut first_id = String::new();
    let mut first_cursor = String::new();
    for expected in [200, 200, 3] {
        let response = app
            .clone()
            .oneshot(request("GET", &path, &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-store"
        );
        let body = json_body(response).await;
        let items = body["items"].as_array().unwrap();
        assert_eq!(items.len(), expected);
        for item in items {
            assert_eq!(
                item.as_object().unwrap().len(),
                3,
                "the list contains an identity, name and lightweight image reference"
            );
            assert!(item["order_image_order_id"].is_string());
            assert!(item.get("code").is_none());
            assert!(item.get("qolip_code").is_none());
            assert!(seen.insert(item["id"].as_str().unwrap().to_owned()));
        }
        if first_id.is_empty() {
            first_id = items[0]["id"].as_str().unwrap().to_owned();
            assert_eq!(items[0]["name"], items[1]["name"]);
            assert_ne!(items[0]["id"], items[1]["id"]);
            assert_eq!(items[0]["order_image_order_id"], "catalog-order-image");
            assert_eq!(items[1]["order_image_order_id"], "");
            first_cursor = body["next_cursor"].as_str().unwrap().to_owned();
        }
        if let Some(cursor) = body["next_cursor"].as_str() {
            path = format!("{base}&cursor={cursor}");
            assert_eq!(body["has_more"], true);
        } else {
            assert_eq!(body["has_more"], false);
        }
    }
    assert_eq!(seen.len(), 403);
    let detail = format!("/v1/mobile/qolip/products/detail?id={first_id}");
    let response = app
        .clone()
        .oneshot(request("GET", &detail, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["products"].as_array().unwrap().len(), 205);
    assert_eq!(
        body["products"][0]["order_image_order_id"],
        "catalog-order-image"
    );
    assert!(
        body["products"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["code"] == "ITEM-000")
    );

    let response = app
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/qolip/products/page?q=Product%20401",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(
        json_body(response).await["items"].as_array().unwrap().len(),
        1
    );
    for (path, actor, status) in [
        (base.to_owned(), "invalid", StatusCode::UNAUTHORIZED),
        (base.to_owned(), denied.as_str(), StatusCode::FORBIDDEN),
        (
            format!("{base}&cursor=bad"),
            token.as_str(),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("{base}&q=different&cursor={first_cursor}"),
            token.as_str(),
            StatusCode::BAD_REQUEST,
        ),
        (
            "/v1/mobile/qolip/products/detail?id=bad".into(),
            token.as_str(),
            StatusCode::BAD_REQUEST,
        ),
        (
            "/v1/mobile/qolip/products/detail?id=qp1.SElEREVO".into(),
            token.as_str(),
            StatusCode::NOT_FOUND,
        ),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request("GET", &path, actor))
                .await
                .unwrap()
                .status(),
            status,
            "{path}"
        );
    }
    // A local write returns exactly its authoritative catalog changes.
    let mutation = |method: &str, path: &str, body: serde_json::Value| {
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let response = app.clone().oneshot(mutation("POST", "/v1/mobile/qolip/product-molds",
        serde_json::json!({"changes": [{"item_code": "ITEM-002", "remove": ["ITEM-002-MOLD-1"]}]})))
        .await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(
        body["catalog_delta"]["upserts"].as_array().unwrap().len(),
        0
    );
    assert_eq!(
        body["catalog_delta"]["removed_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let response = app
        .clone()
        .oneshot(mutation(
            "POST",
            "/v1/mobile/qolip/product-molds",
            serde_json::json!({"changes": [{"item_code": "ITEM-002", "warehouse": "Molds A",
            "add": [{"qolip_code": "ITEM-002-MOLD-1", "existing_only": true}]}]}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let rows = body["catalog_delta"]["upserts"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "Product 002");
    assert_eq!(rows[0]["products"].as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["products"][0]["qolip_code"], "ITEM-002-MOLD-1");

    let response = app
        .clone()
        .oneshot(mutation(
            "DELETE",
            "/v1/mobile/qolip/product-specs",
            serde_json::json!({"qolip_codes": ["ITEM-003-MOLD-1"]}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["deleted_count"], 1);
    assert_eq!(
        body["catalog_delta"]["removed_ids"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    sqlx::query("DELETE FROM mini_warehouse_assignments WHERE principal_ref = 'owner-a'")
        .execute(pool)
        .await
        .unwrap();
    let response = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("{base}&cursor={first_cursor}"),
            &token,
        ))
        .await
        .unwrap();
    assert!(
        json_body(response).await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        app.oneshot(request("GET", &detail, &token))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn qolip_catalog_postgres_pages_match_visible_inventory_and_details_are_unbounded() {
    use mini_rs_erp::db::postgres::apply_postgres_migrations_through_version;
    use mini_rs_erp::db::postgres_qolip::PostgresQolipStore;
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .expect("isolated PostgreSQL test admin URL");
    let database = format!("qolip_catalog_{:032x}", rand::random::<u128>());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE \"{database}\""))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(
        url.parse::<sqlx::postgres::PgConnectOptions>()
            .unwrap()
            .database(&database),
    )
    .await
    .unwrap();
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    for migration in [
        include_str!("../migrations/postgres/0136_qolip_alternative_sets.sql"),
        include_str!("../migrations/postgres/0137_qolip_set_integrity.sql"),
        include_str!("../migrations/postgres/0147_qolip_shared_sets.sql"),
        include_str!("../migrations/postgres/0148_qolip_product_overrides.sql"),
    ] {
        sqlx::raw_sql(migration).execute(&pool).await.unwrap();
    }
    sqlx::raw_sql(r#"
        INSERT INTO mini_item_groups (name) VALUES ('All Item Groups') ON CONFLICT DO NOTHING;
        INSERT INTO mini_item_groups (name, parent_item_group)
            VALUES ('Tayyor mahsulot', 'All Item Groups') ON CONFLICT DO NOTHING;
        INSERT INTO mini_items (code, name, item_group)
        SELECT 'ITEM-' || lpad(n::text, 3, '0'),
            CASE WHEN n < 2 THEN 'Duplicate name' ELSE 'Product ' || lpad(n::text, 3, '0') END,
            'Tayyor mahsulot' FROM generate_series(0, 401) n;
        INSERT INTO mini_items (code, name, item_group) VALUES ('HIDDEN', 'Hidden product', 'Tayyor mahsulot');
        INSERT INTO mini_production_maps (id, product_code, title, map_json, updated_at) VALUES
            ('catalog-order-image', 'ITEM-000', 'Product image', '{"image_id":"catalog-image"}', now() - interval '1 day'),
            ('catalog-order-no-image', 'ITEM-000', 'Newer order without image', '{}', now()),
            ('hidden-order-image', 'HIDDEN', 'Hidden product image', '{"image_id":"hidden-image"}', now());
        INSERT INTO mini_warehouses (id, name, parent_warehouse) VALUES
            ('owner-a', 'Molds A', ''), ('owner-b', 'Molds B', '');
        INSERT INTO mini_warehouse_assignments
            (warehouse, warehouse_name, assignment_kind, principal_role, principal_ref)
        VALUES ('Molds A', 'Molds A', 'warehouse', 'qolipchi', 'owner-a');
        INSERT INTO mini_qolip_product_specs
            (item_code, item_name, item_group, qolip_code, size, payload_json)
        SELECT item.code, item.name, item.item_group,
            item.code || '-MOLD-' || mold, 40,
            jsonb_build_object('warehouse', 'Molds A', 'qolip_set_id', 'set-' || item.code)
        FROM mini_items item
        CROSS JOIN LATERAL generate_series(1, CASE WHEN item.code = 'ITEM-000' THEN 205 ELSE 1 END) mold
        WHERE item.code <> 'HIDDEN';
        INSERT INTO mini_qolip_product_specs
            (item_code, item_name, item_group, qolip_code, size, payload_json)
        VALUES ('HIDDEN', 'Hidden product', 'Tayyor mahsulot', 'SECRET-MOLD', 40, '{"warehouse":"Molds B"}');
        INSERT INTO mini_qolip_set_products (set_id, item_code, created_by_role, created_by_ref, created_by_name)
            VALUES ('set-ITEM-000', 'ITEM-001', 'admin', 'admin', 'Admin');
        INSERT INTO mini_qolip_locations
            (id, block, warehouse, item_code, item_name, qolip_code, size, quantity, created_by_role, created_by_ref)
        VALUES ('legacy', 'A', 'Molds A', 'LEGACY-ITEM', 'Legacy product', 'LEGACY-MOLD', 40, 1, 'qolipchi', 'owner-a');
        INSERT INTO mini_qolip_checkouts
            (id, location_id, block, warehouse, item_code, item_name, qolip_code, size, quantity,
             issued_to_ref, issued_to_name, issued_by_role, issued_by_ref)
        VALUES ('locked', 'unused', 'A', 'Molds A', 'ITEM-000', 'Duplicate name',
            'ITEM-000-MOLD-1', 40, 1, 'worker', 'Worker', 'qolipchi', 'owner-a');
    "#).execute(&pool).await.unwrap();
    let store = PostgresQolipStore::new(pool.clone());
    for allowed in [
        None,
        Some(vec!["Molds A".to_owned()]),
        Some(vec!["Molds B".to_owned()]),
        Some(Vec::new()),
    ] {
        let expected = store
            .products("", 20000, true, allowed.as_deref())
            .await
            .unwrap()
            .into_iter()
            .map(|product| product.code.to_lowercase())
            .collect::<std::collections::BTreeSet<_>>();
        let mut seen = std::collections::BTreeSet::new();
        let mut after = None;
        loop {
            let page = store
                .catalog_page("", 200, after.as_ref(), allowed.as_deref())
                .await
                .unwrap();
            if page.is_empty() {
                break;
            }
            assert!(page.len() <= 200);
            for item in &page {
                assert!(seen.insert(item.code.to_lowercase()));
            }
            let last = page.last().unwrap();
            after = Some(mini_rs_erp::core::qolip::QolipCatalogPosition {
                name: last.name.to_lowercase(),
                code: last.code.to_lowercase(),
            });
        }
        assert_eq!(
            seen, expected,
            "names must cover exactly the existing warehouse-scoped inventory"
        );
    }
    let allowed = vec!["Molds A".to_owned()];
    let detail = store
        .catalog_detail("ITEM-000", Some(&allowed))
        .await
        .unwrap();
    assert_eq!(detail.len(), 205);
    assert!(
        detail
            .iter()
            .any(|mold| mold.qolip_code == "ITEM-000-MOLD-1" && mold.is_in_use)
    );
    assert_eq!(
        store
            .catalog_detail("ITEM-001", Some(&allowed))
            .await
            .unwrap()
            .len(),
        206
    );
    assert!(
        store
            .catalog_detail("HIDDEN", Some(&allowed))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .catalog_detail("LEGACY-ITEM", Some(&allowed))
            .await
            .unwrap()
            .len(),
        1
    );
    let search = store
        .catalog_page("Product 401", 201, None, Some(&allowed))
        .await
        .unwrap();
    assert_eq!(search.len(), 1);
    assert_eq!(search[0].code, "ITEM-401");
    sqlx::query("UPDATE mini_qolip_checkouts SET status = 'returned' WHERE id = 'locked'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        store
            .catalog_detail("ITEM-000", Some(&allowed))
            .await
            .unwrap()
            .iter()
            .all(|mold| !mold.is_in_use)
    );
    verify_http_catalog(&pool).await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

fn test_state() -> (AppState, tempfile::TempDir) {
    let files = tempfile::tempdir().unwrap();
    let mut config = AppConfig::from_env().unwrap();
    config.session_store_path = files.path().join("sessions.json");
    config.profile_store_path = files.path().join("profiles.json");
    config.push_token_store_path = files.path().join("push.json");
    let mut state = AppState::verification(config);
    state.sessions = SessionManager::memory(Some(3600));
    (state, files)
}

async fn session_for(state: &AppState, role: PrincipalRole, reference: &str) -> String {
    state
        .sessions
        .create(Principal {
            role,
            display_name: "Catalog reader".into(),
            legal_name: "Catalog reader".into(),
            ref_: reference.into(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .unwrap()
}

fn request(method: &str, path: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}
