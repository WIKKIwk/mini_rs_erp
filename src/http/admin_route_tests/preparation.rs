use super::*;

#[tokio::test]
async fn preparation_reprint_is_limited_to_own_submitted_receipts() {
    use crate::core::apparatus_standard::CanonicalApparatusService;
    use crate::db::postgres::{apply_foundation_migration, apply_postgres_migrations_through_version, postgres_test_database_options};
    use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;
    use crate::db::postgres_gscale_receipt::PostgresGscaleReceiptStore;
    use crate::db::postgres_preparation::PostgresPreparationStore;
    use std::sync::Arc;

    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let db = format!("mini_rs_erp_test_prep_reprint_{:016x}", rand::random::<u64>());
    sqlx::query(&format!(r#"CREATE DATABASE "{db}""#)).execute(&admin).await.unwrap();
    let seed = sqlx::PgPool::connect_with(postgres_test_database_options(&url, &db)).await.unwrap();
    apply_postgres_migrations_through_version(&seed, "0121").await.unwrap();
    CanonicalApparatusService::new(Arc::new(PostgresCanonicalApparatusRepository::new(seed.clone())))
        .bootstrap_factory_defaults().await.unwrap();
    apply_foundation_migration(&seed).await.unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_warehouses(id,name) VALUES ('reprint-w','Other warehouse');
        INSERT INTO mini_items(code,name,item_group) VALUES ('REPRINT-PE','PE PR','Rulon');
        INSERT INTO mini_gscale_receipts(name,status,item_code,warehouse,qty,barcode,payload_json)
        VALUES ('receipt-own','submitted','REPRINT-PE','Other warehouse',23,'QR-OWN',
                   '{"item_name":"PE PR","actor_role":"tayyorlov_masteri","actor_ref":"prep-reprint"}'),
               ('receipt-other','submitted','REPRINT-PE','Other warehouse',17,'QR-OTHER',
                   '{"item_name":"PE PR","actor_role":"tayyorlov_masteri","actor_ref":"other-master"}'),
               ('receipt-draft','draft','REPRINT-PE','Other warehouse',12,'QR-DRAFT',
                   '{"item_name":"PE PR","actor_role":"tayyorlov_masteri","actor_ref":"prep-reprint"}');
        INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,uom,status,source_receipt_id)
        VALUES ('raw:own','Other warehouse','REPRINT-PE','PE PR','QR-OWN',23,'kg','available','receipt-own'),
               ('raw:other','Other warehouse','REPRINT-PE','PE PR','QR-OTHER',17,'kg','available','receipt-other'),
               ('raw:draft','Other warehouse','REPRINT-PE','PE PR','QR-DRAFT',12,'kg','available','receipt-draft');
    "#).execute(&seed).await.unwrap();
    let runtime = sqlx::postgres::PgPoolOptions::new()
        .after_connect(|connection, _| Box::pin(async move {
            sqlx::query("SET ROLE mini_rs_erp").execute(connection).await?;
            Ok(())
        }))
        .connect_with(postgres_test_database_options(&url, &db)).await.unwrap();
    let mut state = test_state();
    state.preparation = Some(PostgresPreparationStore::new(runtime.clone()));
    state.gscale = GscaleService::new().with_receipt_store(Arc::new(PostgresGscaleReceiptStore::new(runtime.clone())));
    let own = session_for(&state, PrincipalRole::TayyorlovMasteri, "prep-reprint").await;
    let other = session_for(&state, PrincipalRole::TayyorlovMasteri, "other-master").await;
    let router = build_router(state.clone());
    let prepared = router.clone().oneshot(request_with_body("POST",
        "/v1/mobile/admin/raw-material-stock/reprint/prepare", &own,
        r#"{"barcode":"QR-OWN","order_id":"zakaz-original"}"#)).await.unwrap();
    assert_eq!(prepared.status(), StatusCode::OK);
    let body = json_body(prepared).await;
    assert_eq!(body["print"]["epc"], "QR-OWN");
    assert_eq!(body["print"]["gross_qty"], 23.0);
    assert_eq!(body["stock"]["source_receipt_id"], "receipt-own");
    let confirm = serde_json::json!({"barcode":"QR-OWN","reprint_id":body["reprint_id"]}).to_string();
    for (token, expected) in [(&other, StatusCode::FORBIDDEN), (&own, StatusCode::OK)] {
        let response = router.clone().oneshot(request_with_body("POST",
            "/v1/mobile/admin/raw-material-stock/reprint/confirm", token, &confirm)).await.unwrap();
        assert_eq!(response.status(), expected);
    }
    for barcode in ["QR-OTHER", "QR-DRAFT"] {
        let response = router.clone().oneshot(request_with_body("POST",
            "/v1/mobile/admin/raw-material-stock/reprint/prepare", &own,
            &serde_json::json!({"barcode":barcode}).to_string())).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let quantity: (i64, String) = sqlx::query_as(
        "SELECT count(*), max(qty)::text FROM mini_raw_material_stock WHERE barcode='QR-OWN'",
    ).fetch_one(&seed).await.unwrap();
    assert_eq!(quantity.0, 1);
    assert!(quantity.1.starts_with("23"));
    drop(router);
    drop(state);
    runtime.close().await;
    seed.close().await;
    sqlx::query(&format!(r#"DROP DATABASE "{db}" WITH (FORCE)"#)).execute(&admin).await.unwrap();
    admin.close().await;
}

#[tokio::test]
async fn preparation_production_journal_requires_master_capability_and_valid_paging() {
    let state = test_state();
    let admin = session(&state, PrincipalRole::Admin).await;
    let master = session(&state, PrincipalRole::TayyorlovMasteri).await;
    let router = build_router(state);
    for path in [
        "/v1/mobile/preparation/production/orders",
        "/v1/mobile/preparation/production/order?order_id=order-1",
        "/v1/mobile/preparation/production/batch?batch_id=batch-1",
    ] {
        let response = router.clone().oneshot(request_with_body("GET",path,"","")).await.unwrap();
        assert_eq!(response.status(),StatusCode::UNAUTHORIZED);
        let response = router.clone().oneshot(request_with_body("GET",path,&admin,"")).await.unwrap();
        assert_eq!(response.status(),StatusCode::FORBIDDEN);
    }
    for path in [
        "/v1/mobile/preparation/production/orders?limit=0",
        "/v1/mobile/preparation/production/orders?limit=101",
        "/v1/mobile/preparation/production/orders?offset=1000001",
        "/v1/mobile/preparation/production/orders?status=invalid",
        "/v1/mobile/preparation/production/order",
        "/v1/mobile/preparation/production/batch",
    ] {
        let response = router.clone().oneshot(request_with_body("GET",path,&master,"")).await.unwrap();
        assert_eq!(response.status(),StatusCode::BAD_REQUEST,"{path}");
    }
}

#[tokio::test]
async fn raw_material_receipt_keeps_preparation_order_scope_and_other_warehouse_access() {
    use crate::core::preparation::MaterialResponsibilityAssign;
    use crate::db::postgres_preparation::PostgresPreparationStore;
    use crate::http::handlers::admin::validate_receipt_order_assignment;
    use sqlx::{PgPool, postgres::PgConnectOptions};

    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").expect("isolated test database URL");
    let admin_db = PgPool::connect(&url).await.unwrap();
    let db = format!("mini_rs_erp_test_prep_receipt_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db} TEMPLATE template0"))
        .execute(&admin_db).await.unwrap();
    let pool = PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&db))
        .await.unwrap();
    crate::db::postgres::apply_foundation_migration(&pool).await.unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_system_users(id,role,name,phone)
            VALUES('prep-receipt','tayyorlov_masteri','Master','901234599');
        INSERT INTO mini_calculate_materials(id,lower_name,payload_json)
            VALUES('receipt-film','receiptfilm','{"id":"receipt-film","name":"ReceiptFilm","active":true}');
        INSERT INTO mini_orders(id,code,order_number,product_name)
            VALUES('zakaz-receipt','zakaz-receipt','8819','P');
        INSERT INTO mini_order_products(id,order_id,product_name,layers_json)
            VALUES('receipt-product','zakaz-receipt','P','[{"material_id":"receipt-film"}]');
    "#).execute(&pool).await.unwrap();
    let preparation = PostgresPreparationStore::new(pool.clone());
    preparation.assign_responsibility(MaterialResponsibilityAssign {
        principal_ref: "prep-receipt".into(), material_id: "receipt-film".into(),
    }).await.unwrap();
    let mut state = test_state();
    state.preparation = Some(preparation);
    assign_warehouse_to_principal(&state, PrincipalRole::MaterialTaminotchi,
        "supplier", "Other W").await;
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state.clone());
    let apparatus = "apparatus:default:bosma_7";
    let response = router.clone().oneshot(request_with_body("PUT",
        "/v1/mobile/admin/production-maps", &token,
        &pechat_order_map_json_with_dims("zakaz-receipt", "Receipt", "8819", apparatus, 7, 765.0),
    )).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = router.oneshot(request_with_body("PUT",
        "/v1/mobile/admin/raw-material-rules", &token,
        &canonical_requirement_set_material_policy_body(apparatus, 1, &["Rulon"], true),
    )).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut actor = Principal {
        role: PrincipalRole::TayyorlovMasteri, ref_: "prep-receipt".into(),
        display_name: "Master".into(), legal_name: String::new(),
        phone: String::new(), avatar_url: String::new(),
    };
    let assignment = validate_receipt_order_assignment(&state, &actor, "zakaz-receipt",
        apparatus, "ROLL-1000", "Other W", Some(1530.0), Some(25.0))
        .await.unwrap().unwrap();
    assert_eq!(assignment["order_id"], "zakaz-receipt");
    assert_eq!(assignment["apparatus"], apparatus);
    assert_eq!(assignment["assigned_by_role"], "tayyorlov_masteri");
    for (order, destination) in [("zakaz-receipt", "Missing W"), ("other-order", "Other W")] {
        assert!(validate_receipt_order_assignment(&state, &actor, order, apparatus,
            "ROLL-1000", destination, Some(1530.0), Some(25.0)).await.is_err());
    }
    actor.role = PrincipalRole::MaterialTaminotchi;
    assert!(validate_receipt_order_assignment(&state, &actor, "zakaz-receipt", apparatus,
        "ROLL-1000", "Other W", Some(1530.0), Some(25.0)).await.is_err());
    drop(state);
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}")).execute(&admin_db).await.unwrap();
    admin_db.close().await;
}

#[tokio::test]
async fn preparation_inventory_assignment_candidates_follow_scope_and_roll_policy() {
    use crate::core::apparatus_standard::CanonicalApparatusService;
    use crate::core::preparation::MaterialResponsibilityAssign;
    use crate::db::postgres::{apply_foundation_migration, apply_postgres_migrations_through_version};
    use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;
    use crate::db::postgres_preparation::PostgresPreparationStore;
    use sqlx::{PgPool, postgres::PgConnectOptions};

    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .expect("isolated test database URL");
    let admin_db = PgPool::connect(&url).await.unwrap();
    let db = format!("mini_rs_erp_test_prep_assignment_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db} TEMPLATE template0"))
        .execute(&admin_db).await.unwrap();
    let pool = PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&db))
        .await.unwrap();
    apply_postgres_migrations_through_version(&pool, "0121").await.unwrap();
    CanonicalApparatusService::new(Arc::new(
        PostgresCanonicalApparatusRepository::new(pool.clone()),
    )).bootstrap_factory_defaults().await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_system_users(id,role,name,phone)
            VALUES('prep-assignment','tayyorlov_masteri','Master','901234599');
        INSERT INTO mini_calculate_materials(id,lower_name,payload_json)
            VALUES('prep-film','prepfilm','{"id":"prep-film","name":"PrepFilm","active":true}');
        INSERT INTO mini_orders(id,code,order_number,product_name) VALUES
            ('zakaz-prep-allowed','1001','1001','Allowed'),
            ('zakaz-prep-denied','1002','1002','Denied');
        INSERT INTO mini_order_products(id,order_id,product_name,layers_json) VALUES
            ('prep-product-allowed','zakaz-prep-allowed','Allowed','[{"material_id":"prep-film"}]'),
            ('prep-product-denied','zakaz-prep-denied','Denied','[{"material_id":"other-film"}]');
    "#).execute(&pool).await.unwrap();
    let preparation = PostgresPreparationStore::new(pool.clone());
    preparation.assign_responsibility(MaterialResponsibilityAssign {
        principal_ref: "prep-assignment".into(), material_id: "prep-film".into(),
    }).await.unwrap();
    let stock = Arc::new(RawMaterialStockLookup::default());
    stock.insert_stock("QR-PREP-WIDE", "ROLL-1000", "CPP 1530/25", 12.0).await;
    let mut state = test_state();
    state.preparation = Some(preparation);
    state.gscale = GscaleService::new().with_receipt_store(stock);
    assign_warehouse_to_principal(&state, PrincipalRole::TayyorlovMasteri,
        "prep-assignment", "Kalidor").await;
    state.admin.upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
        principal_role: PrincipalRole::MaterialTaminotchi,
        principal_ref: "material-assignment".into(),
        role_id: "material_taminotchi".into(),
        assigned_apparatus: vec!["apparatus:default:bosma_7".into()],
        assigned_item_groups: vec!["Rulon".into()],
    }).await.unwrap();
    assign_warehouse_to_principal(&state, PrincipalRole::MaterialTaminotchi,
        "material-assignment", "Kalidor").await;
    let admin = session(&state, PrincipalRole::Admin).await;
    let master = session_for(&state, PrincipalRole::TayyorlovMasteri, "prep-assignment").await;
    let material = session_for(&state, PrincipalRole::MaterialTaminotchi, "material-assignment").await;
    let router = build_router(state.clone());
    let apparatus = "apparatus:default:bosma_7";
    for (id, code) in [("zakaz-prep-allowed", "1001"), ("zakaz-prep-denied", "1002")] {
        let response = router.clone().oneshot(request_with_body("PUT",
            "/v1/mobile/admin/production-maps", &admin,
            &pechat_order_map_json_with_dims(id, id, code, apparatus, 7, 765.0),
        )).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let response = router.clone().oneshot(request_with_body("PUT",
        "/v1/mobile/admin/raw-material-rules", &admin,
        &canonical_requirement_set_material_policy_body(apparatus, 1, &["Rulon"], true),
    )).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let path = "/v1/mobile/admin/raw-material-assignments/candidate-orders?barcode=QR-PREP-WIDE";
    let response = router.clone().oneshot(request("GET", path, &master)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let candidates = json_body(response).await;
    assert_eq!(candidates.as_array().unwrap().len(), 1);
    assert_eq!(candidates[0]["order"]["map"]["id"], "zakaz-prep-allowed");
    let response = router.clone().oneshot(request("GET", path, &material)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(json_body(response).await.as_array().unwrap().is_empty());

    for (order_id, expected) in [
        ("zakaz-prep-denied", StatusCode::FORBIDDEN),
        ("zakaz-prep-allowed", StatusCode::OK),
    ] {
        let body = serde_json::json!({"order_id": order_id, "barcode": "QR-PREP-WIDE", "apparatus": apparatus}).to_string();
        let response = router.clone().oneshot(request_with_body("POST",
            "/v1/mobile/admin/raw-material-assignments", &master, &body,
        )).await.unwrap();
        assert_eq!(response.status(), expected, "{order_id}");
    }
    let response = router.clone().oneshot(request_with_body("DELETE",
        "/v1/mobile/admin/raw-material-assignments", &master,
        r#"{"order_id":"zakaz-prep-allowed","barcode":"QR-PREP-WIDE"}"#,
    )).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    drop(router);
    drop(state);
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}")).execute(&admin_db).await.unwrap();
    admin_db.close().await;
}

#[tokio::test]
async fn preparation_warehouse_assignment_is_admin_only_and_visible_to_master() {
    let state = test_state();
    let admin = session(&state, PrincipalRole::Admin).await;
    let principal = Principal {
        role: PrincipalRole::TayyorlovMasteri,
        display_name: "Tayyorlov masteri".to_string(),
        legal_name: String::new(),
        ref_: "prep-warehouse".to_string(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let master = state.sessions.create(principal.clone()).await.unwrap();
    let payload = serde_json::json!({
        "warehouse": "Tayyorlov ombori",
        "principal_role": "tayyorlov_masteri",
        "principal_ref": principal.ref_,
        "display_name": "Tayyorlov masteri",
    })
    .to_string();

    let denied = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses/assignments",
            &master,
            &payload,
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert!(
        state
            .warehouses
            .warehouse_assignments("")
            .await
            .unwrap()
            .is_empty()
    );

    let created = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/warehouses/assignments",
            &admin,
            &payload,
        ))
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let assignment = json_body(created).await;
    assert_eq!(assignment["principal_role"], "tayyorlov_masteri");
    assert_eq!(assignment["principal_ref"], principal.ref_);

    let listed = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/warehouses/assignments",
            &admin,
        ))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    assert_eq!(json_body(listed).await[0], assignment);
    assert_eq!(
        state
            .warehouses
            .assigned_warehouse_keys(&principal)
            .await
            .unwrap(),
        vec!["Tayyorlov ombori".to_string()],
    );
}

#[tokio::test]
async fn preparation_system_role_create_login_list_scope_and_fail_closed() {
    let mut state = test_state();
    state.preparation = None;
    let admin = session(&state, PrincipalRole::Admin).await;
    let created=build_router(state.clone()).oneshot(request_with_body("POST","/v1/mobile/admin/system-users",&admin,
        r#"{"id":"prep_test","role":"tayyorlov_masteri","name":"Tayyorlov masteri","phone":"+998901112290"}"#)).await.unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let code = build_router(state.clone())
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/system-users/code/regenerate?id=prep_test",
            &admin,
        ))
        .await
        .unwrap();
    assert_eq!(code.status(), StatusCode::OK);
    let code = json_body(code).await["code"].as_str().unwrap().to_string();
    assert!(code.starts_with("90"));
    let login = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/auth/login",
            "",
            &format!(r#"{{"phone":"+998901112290","code":"{code}"}}"#),
        ))
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    let body = json_body(login).await;
    assert_eq!(body["profile"]["role"], "tayyorlov_masteri");
    assert_eq!(
        body["capabilities"],
        serde_json::json!([
            "preparation.access",
            "gscale.catalog.read",
            "gscale.print",
            "rps.batch.manage",
            "raw_material.assign",
        ])
    );
    let token = body["token"].as_str().unwrap();
    let list = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=tayyorlov_masteri",
            &admin,
        ))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    assert_eq!(
        json_body(list).await["items"][0]["principal_role"],
        "tayyorlov_masteri"
    );
    let missing = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/preparation/snapshot", token))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::SERVICE_UNAVAILABLE);
    for role in [
        PrincipalRole::MaterialTaminotchi,
        PrincipalRole::Boyoqchi,
        PrincipalRole::Customer,
        PrincipalRole::Admin,
    ] {
        let token = session(&state, role).await;
        let denied = build_router(state.clone())
            .oneshot(request("GET", "/v1/mobile/preparation/snapshot", &token))
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    }
    let denied = build_router(state)
        .oneshot(request("GET", "/v1/mobile/preparation/snapshot", ""))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}
