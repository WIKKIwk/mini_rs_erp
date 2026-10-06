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
    let database = format!("mini_rs_erp_test_qolip_sets_{}", std::process::id());
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
    apply_foundation_migration(&pool).await.unwrap();
    sqlx::raw_sql(r#"
        INSERT INTO mini_item_groups(name,parent_item_group,is_group) VALUES ('Set products','All Item Groups',false);
        INSERT INTO mini_items(code,name,item_group) VALUES ('ITEM-SET','Set product','Set products');
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
    "#).execute(&pool).await.unwrap();
    (pool, admin, database)
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

    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
#[ignore = "audit regression: requires an isolated PostgreSQL test URL"]
async fn qolip_alternative_sets_database_rejects_reusing_a_set_across_warehouse_owners() {
    let (pool, admin, database) = fixture().await;
    let runtime = PgPool::connect_with(
        pool.connect_options()
            .as_ref()
            .clone()
            .username("mini_rs_erp"),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE mini_qolip_product_specs SET size = 41 WHERE qolip_code = 'SET-B1'")
        .execute(&runtime)
        .await
        .expect("runtime role can edit an owned catalog row");
    let result = sqlx::query("UPDATE mini_qolip_product_specs SET payload_json = jsonb_set(payload_json, '{qolip_set_id}', '\"set-a\"') WHERE qolip_code = 'SET-B1'")
        .execute(&runtime).await;
    let rejected = result.is_err();
    runtime.close().await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    assert!(
        rejected,
        "DB accepted set-a in two different warehouses; set ownership must be constrained independently of application code"
    );
}
