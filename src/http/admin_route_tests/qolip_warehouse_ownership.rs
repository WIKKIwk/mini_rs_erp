use super::*;
use crate::core::qolip::{QolipService, QolipStorePort};
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_qolip::PostgresQolipStore;

const OWNERSHIP_MIGRATION: &str =
    include_str!("../../../migrations/postgres/0094_qolip_warehouse_ownership.sql");

#[tokio::test]
async fn qolip_warehouse_ownership_backfills_history_and_enforces_api_isolation() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = format!("mini_rs_erp_test_qolip_owner_{}", std::process::id());
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    // A dedicated, disposable test database; never the running ERP database.
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop stale test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &db_name))
        .await
        .expect("test db");
    apply_foundation_migration(&pool).await.expect("migrations");
    sqlx::raw_sql(r#"
        INSERT INTO mini_warehouses(id,name,parent_warehouse) VALUES
            ('owner-a','North molds',''), ('owner-b','Flexo molds',''),
            ('block-a','A','North molds'), ('block-b','B','Flexo molds');
        INSERT INTO mini_warehouse_assignments
            (warehouse,warehouse_name,assignment_kind,principal_role,principal_ref) VALUES
            ('North molds','North molds','warehouse','qolipchi','owner-a'),
            ('Flexo molds','Flexo molds','warehouse','qolipchi','owner-b');
        DROP TRIGGER mini_qolip_specs_persist_warehouse ON mini_qolip_product_specs;
        INSERT INTO mini_qolip_product_specs
            (item_code,item_name,item_group,qolip_code,size,created_by_role,created_by_ref,payload_json) VALUES
            ('ITEM','Product','Tayyor mahsulot','OLD-A',40,'qolipchi','owner-a','{"color":"Red"}'),
            ('ITEM','Product','Tayyor mahsulot','OLD-B',40,'qolipchi','owner-b','{}'),
            ('ITEM','Product','Tayyor mahsulot','PLACED-A',40,'qolipchi','owner-b','{}'),
            ('ITEM','Product','Tayyor mahsulot','ISSUED-B',40,'qolipchi','owner-a','{}'),
            ('ITEM','Product','Tayyor mahsulot','UNKNOWN',40,'qolipchi','missing-owner','{}');
        INSERT INTO mini_qolip_locations
            (id,block,warehouse,item_code,item_name,qolip_code,size,quantity) VALUES
            ('placed-a','A','North molds','ITEM','Product','PLACED-A',40,1),
            ('legacy-b','B','Flexo molds','ITEM','Product','LEGACY-B',40,1);
        UPDATE mini_qolip_locations SET created_by_role='qolipchi',created_by_ref='owner-b' WHERE id='legacy-b';
        INSERT INTO mini_qolip_checkouts
            (id,location_id,block,warehouse,item_code,item_name,qolip_code,size,quantity,issued_to_ref,issued_to_name)
            VALUES ('issued-b','old-location','B','Flexo molds','ITEM','Product','ISSUED-B',40,1,'worker','Worker');
    "#).execute(&pool).await.expect("pre-migration fixtures");
    sqlx::raw_sql(OWNERSHIP_MIGRATION)
        .execute(&pool)
        .await
        .expect("automatic backfill");
    sqlx::raw_sql(OWNERSHIP_MIGRATION)
        .execute(&pool)
        .await
        .expect("idempotent backfill");

    let store = Arc::new(PostgresQolipStore::new(pool.clone()));
    for (code, warehouse) in [
        ("OLD-A", "North molds"),
        ("OLD-B", "Flexo molds"),
        ("PLACED-A", "Flexo molds"),
        ("ISSUED-B", "North molds"),
        ("LEGACY-B", "Flexo molds"),
    ] {
        let spec = store
            .product_spec_by_qolip_code(code)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(spec.warehouse, warehouse, "{code}: historic owner");
    }
    assert_eq!(
        store
            .product_spec_by_qolip_code("OLD-A")
            .await
            .unwrap()
            .unwrap()
            .color,
        "Red"
    );
    assert_eq!(store.product_specs("ITEM").await.unwrap().len(), 6);

    let mut state = test_state();
    state.qolip = QolipService::new(store.clone());
    let admin = session(&state, PrincipalRole::Admin).await;
    let a = session_for(&state, PrincipalRole::Qolipchi, "owner-a").await;
    let b = session_for(&state, PrincipalRole::Qolipchi, "owner-b").await;
    let unassigned = session_for(&state, PrincipalRole::Qolipchi, "unassigned").await;
    let router = build_router(state);
    for (token, expected) in [
        (&a, vec!["ISSUED-B", "OLD-A"]),
        (&b, vec!["LEGACY-B", "OLD-B", "PLACED-A"]),
        (&unassigned, vec![]),
    ] {
        let response = router
            .clone()
            .oneshot(request(
                "GET",
                "/v1/mobile/qolip/products?with_qolip_only=true",
                token,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = json_body(response).await;
        let mut actual: Vec<_> = body["products"]
            .as_array()
            .unwrap()
            .iter()
            .map(|product| product["qolip_code"].as_str().unwrap())
            .collect();
        actual.sort();
        assert_eq!(actual, expected);
        for product in body["products"].as_array().unwrap() {
            assert!(expected.contains(&product["first_qolip_code"].as_str().unwrap()));
        }
    }
    let all = json_body(
        router
            .clone()
            .oneshot(request(
                "GET",
                "/v1/mobile/qolip/products?with_qolip=true",
                &admin,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(all["products"].as_array().unwrap().len(), 6);

    // Incorrect legacy placement does not change ownership or expose the mold.
    let misplaced = json_body(
        router
            .clone()
            .oneshot(request("GET", "/v1/mobile/qolip/scan?qr=PLACED-A", &b))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(misplaced["product"]["warehouse"], "Flexo molds");
    assert!(misplaced["location"].is_null());
    let block_a = json_body(
        router
            .clone()
            .oneshot(request("GET", "/v1/mobile/qolip/locations?block=A", &a))
            .await
            .unwrap(),
    )
    .await;
    assert!(block_a["locations"].as_array().unwrap().is_empty());

    let create = r#"{"item_code":"ITEM","item_name":"Product","item_group":"Tayyor mahsulot","qolip_code":"NEW-A","size":40}"#;
    let response = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/product-specs",
            &a,
            create,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["product"]["warehouse"],
        "North molds"
    );
    assert_eq!(
        store
            .product_spec_by_qolip_code("NEW-A")
            .await
            .unwrap()
            .unwrap()
            .warehouse,
        "North molds"
    );

    for (method, path, body) in [
        ("GET", "/v1/mobile/qolip/scan?qr=OLD-B", ""),
        ("GET", "/v1/mobile/qolip/scan?qr=UNKNOWN", ""),
        (
            "POST",
            "/v1/mobile/qolip/code-qr/print",
            r#"{"qolip_code":"OLD-B","print_transport":"offline"}"#,
        ),
        (
            "DELETE",
            "/v1/mobile/qolip/product-specs",
            r#"{"qolip_codes":["OLD-A","OLD-B"]}"#,
        ),
        (
            "POST",
            "/v1/mobile/qolip/product-specs",
            r#"{"previous_qolip_code":"OLD-B","qolip_code":"STOLEN","item_code":"ITEM","item_name":"Product","item_group":"Tayyor mahsulot","size":40}"#,
        ),
        (
            "POST",
            "/v1/mobile/qolip/product-specs",
            r#"{"warehouse":"Flexo molds","qolip_code":"SPOOFED","item_code":"ITEM","item_name":"Product","item_group":"Tayyor mahsulot","size":40}"#,
        ),
        (
            "POST",
            "/v1/mobile/qolip/product-specs/batch",
            r#"{"specs":[{"warehouse":"North molds","qolip_code":"BATCH-A","item_code":"ITEM","item_name":"Product","item_group":"Tayyor mahsulot","size":40},{"warehouse":"Flexo molds","qolip_code":"BATCH-B","item_code":"ITEM","item_name":"Product","item_group":"Tayyor mahsulot","size":40}]}"#,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(method, path, &a, body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
    }
    assert!(
        store
            .product_spec_by_qolip_code("OLD-A")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .product_spec_by_qolip_code("BATCH-A")
            .await
            .unwrap()
            .is_none()
    );
    let place_foreign = router.clone().oneshot(request_with_body("POST", "/v1/mobile/qolip/locations", &a,
        r#"{"block":"A","warehouse":"North molds","item_code":"ITEM","item_name":"Product","qolip_code":"OLD-B","size":40,"quantity":1,"row_letter":"A","column_number":1}"#)).await.unwrap();
    assert_eq!(place_foreign.status(), StatusCode::CONFLICT);

    // Renaming/editing and subsequent staff reassignment must not move a mold's owner.
    let renamed = router.clone().oneshot(request_with_body("POST", "/v1/mobile/qolip/product-specs", &a,
        r#"{"previous_qolip_code":"NEW-A","qolip_code":"RENAMED-A","item_code":"ITEM","item_name":"Product","item_group":"Tayyor mahsulot","size":41}"#)).await.unwrap();
    assert_eq!(renamed.status(), StatusCode::OK);
    assert_eq!(
        json_body(renamed).await["product"]["warehouse"],
        "North molds"
    );
    sqlx::query("UPDATE mini_warehouse_assignments SET warehouse_name='Flexo molds' WHERE principal_ref='owner-a'")
        .execute(&pool).await.unwrap();
    assert_eq!(
        store
            .product_spec_by_qolip_code("OLD-A")
            .await
            .unwrap()
            .unwrap()
            .warehouse,
        "North molds"
    );
    assert!(sqlx::query("UPDATE mini_qolip_product_specs SET payload_json=jsonb_set(payload_json,'{warehouse}','\"Flexo molds\"') WHERE qolip_code='OLD-A'")
        .execute(&pool).await.is_err(), "DB must reject accidental owner overwrite");
    assert!(sqlx::query("INSERT INTO mini_qolip_product_specs(item_code,item_name,qolip_code,size) VALUES ('X','X','NO-OWNER',40)")
        .execute(&pool).await.is_err(), "new molds cannot be saved ownerless");

    drop(router);
    drop(store);
    pool.close().await;
    sqlx::query(&format!(r#"DROP DATABASE "{db_name}" WITH (FORCE)"#))
        .execute(&admin_pool)
        .await
        .expect("remove dedicated test db");
    admin_pool.close().await;
}
