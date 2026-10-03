use super::*;
use crate::core::qolip::{QolipError, QolipService, QolipStorePort};
use crate::db::postgres::{apply_foundation_migration, postgres_test_database_options};
use crate::db::postgres_qolip::PostgresQolipStore;

const PATH: &str = "/v1/mobile/qolip/product-transfer";

#[tokio::test]
async fn qolip_product_transfer_requires_qolipchi_even_for_admin() {
    let state = test_state();
    for role in [
        PrincipalRole::Admin,
        PrincipalRole::Aparatchi,
        PrincipalRole::MaterialTaminotchi,
    ] {
        let token = session(&state, role).await;
        let response = build_router(state.clone())
            .oneshot(request_with_body("POST", PATH, &token, "{}"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{role:?}");
    }
}

#[tokio::test]
#[ignore = "requires local PostgreSQL; creates and drops an isolated database"]
async fn qolip_product_transfer_preserves_qr_stock_history_and_is_atomic_in_postgres() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let database = format!("mini_rs_erp_test_qolip_transfer_{}", std::process::id());
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("test admin database");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{database}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .unwrap();
    sqlx::query(&format!(r#"CREATE DATABASE "{database}""#))
        .execute(&admin_pool)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &database))
        .await
        .unwrap();
    crate::db::postgres::apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    let apparatus =
        crate::core::apparatus_standard::service::CanonicalApparatusService::new(Arc::new(
            crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository::new(
                pool.clone(),
            ),
        ));
    for revision in crate::core::apparatus_standard::test_support::standard_revisions() {
        apparatus
            .seed_for_test(revision.apparatus_id.clone(), revision.to_draft())
            .await
            .unwrap();
    }
    apply_foundation_migration(&pool).await.unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_item_groups(name,parent_item_group,is_group)
            VALUES ('Transfer finished goods','All Item Groups',false);
        INSERT INTO mini_items(code,name,item_group) VALUES
            ('SOURCE','Milano','Transfer finished goods'),
            ('TARGET','Milana','Transfer finished goods');
        INSERT INTO mini_warehouses(id,name,parent_warehouse) VALUES
            ('root-a','Molds A',''), ('root-b','Molds B',''), ('block-a','A','Molds A');
        INSERT INTO mini_warehouse_assignments
            (warehouse,warehouse_name,assignment_kind,principal_role,principal_ref) VALUES
            ('Molds A','Molds A','warehouse','qolipchi','owner-a'),
            ('Molds B','Molds B','warehouse','qolipchi','owner-b');
        INSERT INTO mini_qolip_product_specs
            (item_code,item_name,item_group,qolip_code,size,created_by_role,created_by_ref,created_by_name,payload_json) VALUES
            ('SOURCE','Milano','Transfer finished goods','QR-1',42,'qolipchi','owner-a','Maker', '{"warehouse":"Molds A","color":"Red"}'),
            ('SOURCE','Milano','Transfer finished goods','QR-2',43,'qolipchi','owner-a','Maker', '{"warehouse":"Molds A","color":"Blue"}'),
            ('SOURCE','Milano','Transfer finished goods','QR-ACTIVE',44,'qolipchi','owner-a','Maker', '{"warehouse":"Molds A"}'),
            ('SOURCE','Milano','Transfer finished goods','QR-FOREIGN',45,'qolipchi','owner-b','Maker B', '{"warehouse":"Molds B"}');
        INSERT INTO mini_qolip_locations
            (id,block,warehouse,item_code,item_name,qolip_code,size,quantity,row_letter,column_number,location_label,payload_json) VALUES
            ('location-1','A','Molds A','SOURCE','Milano','QR-1',42,2,'B',3,'B3','{"item_code":"SOURCE","item_name":"Milano"}'),
            ('location-2','A','Molds A','SOURCE','Milano','QR-2',43,1,'B',4,'B4','{}');
        INSERT INTO mini_qolip_checkouts
            (id,location_id,block,warehouse,item_code,item_name,qolip_code,size,quantity,issued_to_ref,issued_to_name,status) VALUES
            ('history-1','location-1','A','Molds A','SOURCE','Milano','QR-1',42,1,'worker','Worker','returned'),
            ('open-2','location-2','A','Molds A','SOURCE','Milano','QR-2',43,1,'worker','Worker','open');
        INSERT INTO mini_production_maps(id,product_code,title,map_json)
            VALUES ('active-order','SOURCE','Milano','{}');
        INSERT INTO mini_order_run_sessions
            (session_id,apparatus,canonical_apparatus_id,order_id,status,payload_json)
            VALUES ('active-session','Bosma 7','apparatus:default:bosma_7','active-order','paused',
                    '{"qolip_lock_owner":true,"qolip_codes":["QR-ACTIVE"]}');
    "#).execute(&pool).await.unwrap();

    let store = Arc::new(PostgresQolipStore::new(pool.clone()));
    let service = QolipService::new(store.clone());
    let mut state = test_state();
    state.qolip = service.clone();
    let owner = session_for(&state, PrincipalRole::Qolipchi, "owner-a").await;
    let unassigned = session_for(&state, PrincipalRole::Qolipchi, "nobody").await;
    let router = build_router(state);
    let body = |id: &str, codes: &[&str]| {
        serde_json::json!({
        "request_id": id, "from_item_code": "SOURCE", "to_item_code": "TARGET", "qolip_codes": codes,
    }).to_string()
    };

    for (token, id, codes, expected) in [
        (
            &unassigned,
            "unassigned",
            vec!["QR-1"],
            StatusCode::FORBIDDEN,
        ),
        (
            &owner,
            "foreign",
            vec!["QR-1", "QR-FOREIGN"],
            StatusCode::FORBIDDEN,
        ),
        (
            &owner,
            "open",
            vec!["QR-1", "QR-2"],
            StatusCode::CONFLICT,
        ),
        (
            &owner,
            "active",
            vec!["QR-1", "QR-ACTIVE"],
            StatusCode::CONFLICT,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body("POST", PATH, token, &body(id, &codes)))
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{id}");
        assert_eq!(
            store
                .product_spec_by_qolip_code("QR-1")
                .await
                .unwrap()
                .unwrap()
                .item_code,
            "SOURCE"
        );
        assert_eq!(
            store
                .location_by_qolip_code("QR-1")
                .await
                .unwrap()
                .unwrap()
                .item_code,
            "SOURCE"
        );
    }
    sqlx::query("UPDATE mini_qolip_checkouts SET status = 'returned' WHERE id = 'open-2'")
        .execute(&pool)
        .await
        .unwrap();
    let transfer = body("successful", &["qr-2", "QR-1"]);
    let response = router
        .clone()
        .oneshot(request_with_body("POST", PATH, &owner, &transfer))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let result = json_body(response).await;
    assert_eq!(result["specs"].as_array().unwrap().len(), 2);
    for (code, size, color, quantity, cell) in
        [("QR-1", 42, "Red", 2, "B3"), ("QR-2", 43, "Blue", 1, "B4")]
    {
        let spec = store
            .product_spec_by_qolip_code(code)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(spec.item_code, "TARGET");
        assert_eq!(spec.item_name, "Milana");
        assert_eq!(spec.qolip_code, code);
        assert_eq!((spec.size, spec.color.as_str()), (size, color));
        assert_eq!(
            (spec.warehouse.as_str(), spec.created_by_ref.as_str()),
            ("Molds A", "owner-a")
        );
        let location = store.location_by_qolip_code(code).await.unwrap().unwrap();
        assert_eq!(
            (location.item_code.as_str(), location.item_name.as_str()),
            ("TARGET", "Milana")
        );
        assert_eq!(
            (
                location.quantity,
                location.location_label.as_str(),
                location.block.as_str()
            ),
            (quantity, cell, "A")
        );
        let payload: serde_json::Value =
            sqlx::query_scalar("SELECT payload_json FROM mini_qolip_locations WHERE id = $1")
                .bind(&location.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(payload["item_code"], "TARGET");
        assert_eq!(payload["item_name"], "Milana");
    }
    let history: (String, String) = sqlx::query_as(
        "SELECT item_code,item_name FROM mini_qolip_checkouts WHERE id = 'history-1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(history, ("SOURCE".into(), "Milano".into()));
    let actor = Principal {
        role: PrincipalRole::Qolipchi,
        ref_: "owner-a".into(),
        display_name: "Maker".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    service
        .prepare_qolip_code_for_order_start("QR-1", "TARGET", "Milana", "worker", "Worker", &actor)
        .await
        .expect("corrected mold can start the correct product");
    assert_eq!(
        service
            .prepare_qolip_code_for_order_start(
                "QR-1", "SOURCE", "Milano", "worker", "Worker", &actor
            )
            .await
            .unwrap_err(),
        QolipError::QolipCodeMismatch
    );

    let replay = router
        .clone()
        .oneshot(request_with_body("POST", PATH, &owner, &transfer))
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(json_body(replay).await, result);
    let collision = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            PATH,
            &owner,
            &body("successful", &["QR-1"]),
        ))
        .await
        .unwrap();
    assert_eq!(collision.status(), StatusCode::CONFLICT);
    let stale = router
        .oneshot(request_with_body(
            "POST",
            PATH,
            &owner,
            &body("stale", &["QR-1"]),
        ))
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_engine_events WHERE domain='qolip' AND action='product_transfer'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(count, 1);
    pool.close().await;
    sqlx::query(&format!(r#"DROP DATABASE "{database}" WITH (FORCE)"#))
        .execute(&admin_pool)
        .await
        .unwrap();
    admin_pool.close().await;
}
