use super::qolip_session_helpers::{reject_qolip_in_use_tx, validate_qolip_set_tx};
use crate::core::production_map::{OrderRunSession, OrderRunStatus, ProductionMapError};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    postgres_test_database_options,
};
use sqlx::PgPool;

async fn fixture() -> (PgPool, PgPool, String) {
    let url =
        std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").expect("isolated PostgreSQL test URL");
    let admin = PgPool::connect(&url).await.unwrap();
    let database = format!(
        "mini_rs_erp_test_qolip_sets_{}_{:08x}",
        std::process::id(),
        rand::random::<u32>()
    );
    sqlx::query(&format!("CREATE DATABASE \"{database}\""))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(postgres_test_database_options(&url, &database))
        .await
        .unwrap();
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    let apparatus = crate::core::apparatus_standard::service::CanonicalApparatusService::new(
        std::sync::Arc::new(
            crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository::new(
                pool.clone(),
            ),
        ),
    );
    for revision in crate::core::apparatus_standard::test_support::standard_revisions() {
        apparatus
            .seed_for_test(revision.apparatus_id.clone(), revision.to_draft())
            .await
            .unwrap();
    }
    apply_postgres_migrations_through_version(&pool, "0136")
        .await
        .unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_item_groups(name,parent_item_group,is_group) VALUES ('Set products','All Item Groups',false);
        INSERT INTO mini_items(code,name,item_group) VALUES
            ('ITEM-SET','Set product','Set products'),
            ('ITEM-OTHER','Other product','Set products'),
            ('ITEM-LEGACY','Legacy product','Set products');
        INSERT INTO mini_warehouses(id,name,parent_warehouse) VALUES ('set-owner-a','Set warehouse A',''), ('set-owner-b','Set warehouse B','');
        INSERT INTO mini_warehouse_assignments(warehouse,warehouse_name,assignment_kind,principal_role,principal_ref) VALUES
            ('Set warehouse A','Set warehouse A','warehouse','qolipchi','set-clerk-a'),
            ('Set warehouse B','Set warehouse B','warehouse','qolipchi','set-clerk-b');
        INSERT INTO mini_qolip_product_specs(item_code,item_name,item_group,qolip_code,size,payload_json) VALUES
            ('ITEM-SET','Set product','Set products','SET-A1',40,'{"warehouse":"Set warehouse A","qolip_set_id":"set-a"}'),
            ('ITEM-SET','Set product','Set products','SET-A2',40,'{"warehouse":"Set warehouse A","qolip_set_id":"set-a"}'),
            ('ITEM-SET','Set product','Set products','SET-B1',40,'{"warehouse":"Set warehouse B","qolip_set_id":"set-b"}'),
            ('ITEM-SET','Set product','Set products','SET-B2',40,'{"warehouse":"Set warehouse B","qolip_set_id":"set-b"}');
        INSERT INTO mini_production_maps(id,product_code,title,map_json) VALUES ('set-order','ITEM-SET','Set product','{}');
        INSERT INTO mini_qolip_product_specs(item_code,item_name,item_group,qolip_code,size,payload_json) VALUES
            ('ITEM-LEGACY','Legacy product','Set products','LEGACY-A1',40,'{"warehouse":"Set warehouse A"}'),
            ('ITEM-LEGACY','Legacy product','Set products','LEGACY-A2',40,'{"warehouse":"Set warehouse A"}');
        -- Reproduce an unresolved receipt that predates the ownership trigger.
        ALTER TABLE mini_qolip_product_specs DISABLE TRIGGER mini_qolip_specs_persist_warehouse;
        INSERT INTO mini_qolip_product_specs(item_code,item_name,qolip_code,size,payload_json)
            VALUES ('ITEM-LEGACY','Legacy product','LEGACY-UNKNOWN',40,'{}');
        ALTER TABLE mini_qolip_product_specs ENABLE TRIGGER mini_qolip_specs_persist_warehouse;
    "#).execute(&pool).await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    (pool, admin, database)
}

async fn cleanup(pool: PgPool, admin: PgPool, database: String) {
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

async fn runtime_pool(pool: &PgPool) -> PgPool {
    PgPool::connect_with(
        pool.connect_options()
            .as_ref()
            .clone()
            .username("mini_rs_erp"),
    )
    .await
    .unwrap()
}

fn assert_sqlstate(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>, code: &str) {
    let error = result.expect_err("database must reject an invalid write");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some(code),
        "{error}"
    );
}

fn session(id: &str, set_id: &str, codes: &[&str]) -> OrderRunSession {
    OrderRunSession {
        session_id: id.into(),
        apparatus: "apparatus:default:bosma_7".into(),
        order_id: "set-order".into(),
        stage_node_id: "bosma_7".into(),
        status: OrderRunStatus::Active,
        worker_role: "aparatchi".into(),
        worker_ref: id.into(),
        worker_display_name: id.into(),
        started_at_unix: 1,
        updated_at_unix: 1,
        payload_json: serde_json::json!({"qolip_lock_owner": true, "qolip_set_id": set_id, "qolip_codes": codes}),
    }
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL test URL"]
async fn qolip_alternative_sets_validate_membership_and_serialize_competing_starts_in_postgres() {
    let (pool, admin, database) = fixture().await;
    let catalog = crate::core::qolip::QolipService::new(std::sync::Arc::new(
        crate::db::postgres_qolip::PostgresQolipStore::new(pool.clone()),
    ));
    for (clerk, expected) in [
        ("set-clerk-a", vec!["SET-A1", "SET-A2"]),
        ("set-clerk-b", vec!["SET-B1", "SET-B2"]),
        ("unassigned", vec![]),
    ] {
        let principal = crate::core::auth::models::Principal {
            role: crate::core::auth::models::PrincipalRole::Qolipchi,
            ref_: clerk.into(),
            display_name: clerk.into(),
            legal_name: clerk.into(),
            phone: String::new(),
            avatar_url: String::new(),
        };
        let products = catalog
            .products_for_principal(&principal, false, "ITEM-SET", 100, true)
            .await
            .unwrap();
        let codes: Vec<_> = products
            .iter()
            .map(|product| product.qolip_code.as_str())
            .collect();
        assert_eq!(codes, expected, "{clerk}: only its warehouse molds");
        let identities = catalog
            .visible_order_products(&["ITEM-SET".into()], &principal)
            .await
            .unwrap();
        assert_eq!(identities[0].has_qolip_spec, clerk != "unassigned");
        assert!(identities[0].qolip_code.is_empty());
        assert!(identities[0].qolip_set_id.is_empty());
    }
    for (id, codes, accepted) in [
        ("set-a", vec!["SET-A1", "set-a2", "SET-A1"], true),
        ("set-b", vec!["SET-B1", "SET-B2"], true),
        ("set-a", vec!["SET-A1"], false),
        ("set-a", vec!["SET-A1", "SET-B2"], false),
        ("set-a", vec!["SET-A1", "SET-A2", "SET-B1", "SET-B2"], false),
        ("missing", vec!["SET-A1", "SET-A2"], false),
    ] {
        let mut tx = pool.begin().await.unwrap();
        let result = validate_qolip_set_tx(&mut tx, &session("validation", id, &codes)).await;
        assert_eq!(result.is_ok(), accepted, "{id}: {codes:?}");
        if !accepted {
            assert_eq!(result, Err(ProductionMapError::QolipCodeMismatch));
        }
        tx.rollback().await.unwrap();
    }

    let first = session("first", "set-a", &["SET-A1", "SET-A2"]);
    let mut tx = pool.begin().await.unwrap();
    reject_qolip_in_use_tx(&mut tx, &first).await.unwrap();
    validate_qolip_set_tx(&mut tx, &first).await.unwrap();
    sqlx::query("INSERT INTO mini_order_run_sessions(session_id,apparatus,canonical_apparatus_id,order_id,status,worker_role,worker_ref,payload_json)
        VALUES ('first','Bosma 7','apparatus:default:bosma_7','set-order','active','aparatchi','first',$1)")
        .bind(&first.payload_json).execute(&mut *tx).await.unwrap();
    let contender_pool = pool.clone();
    let mut competing = tokio::spawn(async move {
        let mut tx = contender_pool.begin().await.unwrap();
        let result =
            reject_qolip_in_use_tx(&mut tx, &session("second", "set-a", &["SET-A1", "SET-A2"]))
                .await;
        tx.rollback().await.unwrap();
        result
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(150), &mut competing)
            .await
            .is_err()
    );
    tx.commit().await.unwrap();
    assert_eq!(
        competing.await.unwrap(),
        Err(ProductionMapError::QolipAlreadyInUse)
    );
    let mut tx = pool.begin().await.unwrap();
    let other = session("other-set", "set-b", &["SET-B1", "SET-B2"]);
    reject_qolip_in_use_tx(&mut tx, &other).await.unwrap();
    validate_qolip_set_tx(&mut tx, &other).await.unwrap();
    tx.rollback().await.unwrap();

    cleanup(pool, admin, database).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL test URL"]
async fn qolip_alternative_sets_database_rejects_reusing_a_set_across_warehouse_owners() {
    let (pool, admin, database) = fixture().await;
    let runtime = runtime_pool(&pool).await;
    sqlx::query("UPDATE mini_qolip_product_specs SET size = 41 WHERE qolip_code = 'SET-B1'")
        .execute(&runtime)
        .await
        .expect("runtime role can edit an owned catalog row");
    assert_sqlstate(sqlx::query("UPDATE mini_qolip_product_specs SET payload_json = jsonb_set(payload_json, '{qolip_set_id}', '\"set-a\"') WHERE qolip_code = 'SET-B1'")
        .execute(&runtime).await, "23503");
    assert_sqlstate(sqlx::query("INSERT INTO mini_qolip_product_specs(item_code,item_name,qolip_code,size,payload_json)
        VALUES ('ITEM-OTHER','Other product','INVALID-PRODUCT',40,'{\"warehouse\":\"Set warehouse A\",\"qolip_set_id\":\"set-a\"}')")
        .execute(&runtime).await, "23503");
    assert_sqlstate(sqlx::query("INSERT INTO mini_qolip_product_specs(item_code,item_name,qolip_code,size,payload_json)
        VALUES ('ITEM-SET','Set product','INVALID-OWNER',40,'{\"warehouse\":\"Set warehouse B\",\"qolip_set_id\":\"set-a\"}')")
        .execute(&runtime).await, "23503");
    for statement in [
        "UPDATE mini_qolip_sets SET warehouse_key = 'set warehouse b' WHERE set_id = 'set-a'",
        "DELETE FROM mini_qolip_sets WHERE set_id = 'set-a'",
        "INSERT INTO mini_qolip_sets(set_id,item_code_key,warehouse_key) VALUES ('forged','item-set','set warehouse a')",
    ] {
        assert_sqlstate(sqlx::query(statement).execute(&runtime).await, "42501");
    }
    runtime.close().await;
    cleanup(pool, admin, database).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL test URL"]
async fn qolip_alternative_sets_backfill_and_product_corrections_keep_batch_boundaries() {
    let (pool, admin, database) = fixture().await;
    let legacy: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT qolip_set_id FROM mini_qolip_product_specs WHERE qolip_code LIKE 'LEGACY-%' ORDER BY qolip_code",
    ).fetch_all(&pool).await.unwrap();
    assert_eq!(
        legacy,
        vec![
            Some("legacy:11:item-legacy:set warehouse a".into()),
            Some("legacy:11:item-legacy:set warehouse a".into()),
            None,
        ]
    );
    let runtime = runtime_pool(&pool).await;
    sqlx::query("UPDATE mini_qolip_product_specs SET payload_json = payload_json - 'qolip_set_id' WHERE qolip_code = 'SET-A1'")
        .execute(&runtime).await.unwrap();
    let preserved: String = sqlx::query_scalar(
        "SELECT qolip_set_id FROM mini_qolip_product_specs WHERE qolip_code = 'SET-A1'",
    )
    .fetch_one(&runtime)
    .await
    .unwrap();
    assert_eq!(preserved, "set-a");
    sqlx::query("UPDATE mini_qolip_product_specs SET item_code = 'ITEM-OTHER' WHERE qolip_code IN ('SET-A1','SET-A2','SET-B1','SET-B2')")
        .execute(&runtime).await.unwrap();
    let corrected: Vec<String> = sqlx::query_scalar("SELECT qolip_set_id FROM mini_qolip_product_specs WHERE item_code = 'ITEM-OTHER' ORDER BY qolip_code")
        .fetch_all(&runtime).await.unwrap();
    assert_eq!(corrected[0], corrected[1]);
    assert_eq!(corrected[2], corrected[3]);
    assert_ne!(corrected[0], corrected[2]);
    assert_ne!(corrected[0], "set-a");
    assert_ne!(corrected[2], "set-b");
    // Even with no remaining members, a historic ID keeps its original scope.
    assert_sqlstate(sqlx::query("INSERT INTO mini_qolip_product_specs(item_code,item_name,qolip_code,size,payload_json)
        VALUES ('ITEM-SET','Set product','INVALID-REUSE',40,'{\"warehouse\":\"Set warehouse B\",\"qolip_set_id\":\"set-a\"}')")
        .execute(&runtime).await, "23503");
    runtime.close().await;
    cleanup(pool, admin, database).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL test URL"]
async fn qolip_alternative_sets_concurrent_first_receipts_cannot_split_set_ownership() {
    let (pool, admin, database) = fixture().await;
    let runtime = runtime_pool(&pool).await;
    let mut first = runtime.begin().await.unwrap();
    sqlx::query("INSERT INTO mini_qolip_product_specs(item_code,item_name,qolip_code,size,payload_json)
        VALUES ('ITEM-SET','Set product','CONCURRENT-A1',40,'{\"warehouse\":\"Set warehouse A\",\"qolip_set_id\":\"concurrent-set\"}')")
        .execute(&mut *first).await.unwrap();
    let contender_pool = runtime.clone();
    let mut contender = tokio::spawn(async move {
        sqlx::query("INSERT INTO mini_qolip_product_specs(item_code,item_name,qolip_code,size,payload_json)
            VALUES ('ITEM-SET','Set product','CONCURRENT-B1',40,'{\"warehouse\":\"Set warehouse B\",\"qolip_set_id\":\"concurrent-set\"}')")
            .execute(&contender_pool).await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(150), &mut contender)
            .await
            .is_err()
    );
    first.commit().await.unwrap();
    assert_sqlstate(contender.await.unwrap(), "23503");
    sqlx::query("INSERT INTO mini_qolip_product_specs(item_code,item_name,qolip_code,size,payload_json)
        VALUES ('item-set','Set product','CONCURRENT-A2',40,'{\"warehouse\":\"set WAREHOUSE a\",\"qolip_set_id\":\"concurrent-set\"}')")
        .execute(&runtime).await.unwrap();
    let members: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_qolip_product_specs WHERE qolip_set_id = 'concurrent-set'",
    )
    .fetch_one(&runtime)
    .await
    .unwrap();
    assert_eq!(members, 2);
    runtime.close().await;
    cleanup(pool, admin, database).await;
}

#[tokio::test]
#[ignore = "requires an isolated PostgreSQL test URL"]
async fn qolip_alternative_sets_native_batch_edit_and_transfer_keep_valid_scope() {
    use crate::core::auth::models::{Principal, PrincipalRole};
    use crate::core::qolip::{QolipProductSpecUpsert, QolipProductTransfer, QolipService};
    let (pool, admin, database) = fixture().await;
    let runtime = runtime_pool(&pool).await;
    let catalog = QolipService::new(std::sync::Arc::new(
        crate::db::postgres_qolip::PostgresQolipStore::new(runtime.clone()),
    ));
    let clerk = Principal {
        role: PrincipalRole::Qolipchi,
        ref_: "set-clerk-a".into(),
        display_name: "Clerk A".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let input = |code: &str| QolipProductSpecUpsert {
        warehouse: "Set warehouse A".into(),
        item_code: "ITEM-SET".into(),
        item_name: "Set product".into(),
        item_group: "Set products".into(),
        qolip_code: code.into(),
        size: 42,
        ..Default::default()
    };
    let batch = catalog
        .upsert_product_specs(vec![input("NATIVE-C1"), input("NATIVE-C2")], &clerk)
        .await
        .unwrap();
    assert_eq!(batch[0].set_id(), batch[1].set_id());
    let edited = catalog
        .upsert_product_spec(
            QolipProductSpecUpsert {
                previous_qolip_code: "NATIVE-C1".into(),
                size: 43,
                ..input("NATIVE-C1")
            },
            &clerk,
        )
        .await
        .unwrap();
    assert_eq!(edited.set_id(), batch[0].set_id());
    let request = QolipProductTransfer {
        request_id: "native-set-transfer".into(),
        from_item_code: "ITEM-SET".into(),
        to_item_code: "ITEM-OTHER".into(),
        qolip_codes: vec!["NATIVE-C1".into(), "NATIVE-C2".into()],
    };
    let transferred = catalog
        .transfer_product_specs(request.clone(), &clerk)
        .await
        .unwrap();
    assert_eq!(transferred[0].set_id(), transferred[1].set_id());
    assert_ne!(transferred[0].set_id(), batch[0].set_id());
    assert!(
        transferred
            .iter()
            .all(|spec| spec.item_code == "ITEM-OTHER" && spec.warehouse == "Set warehouse A")
    );
    let retried = catalog
        .transfer_product_specs(request, &clerk)
        .await
        .unwrap();
    assert_eq!(retried, transferred);
    runtime.close().await;
    cleanup(pool, admin, database).await;
}
