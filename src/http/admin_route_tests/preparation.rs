use super::*;

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
