use super::*;
use crate::core::auth::models::PrincipalRole;
use crate::core::qolip::{QolipMoldDraft, QolipReworkSend, QolipService};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    canonical_apparatus_service, postgres_test_database_options,
};
use crate::db::postgres_qolip::PostgresQolipStore;
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires an explicit disposable MINI_ERP_TEST_ADMIN_DATABASE_URL"]
async fn qolip_set_names_postgres_persist_and_serialize_case_insensitive_conflicts() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL").unwrap();
    let server = PgPool::connect(&url).await.unwrap();
    let database = format!("qolip_names_{:032x}", rand::random::<u128>());
    sqlx::query(&format!("CREATE DATABASE \"{database}\""))
        .execute(&server)
        .await
        .unwrap();
    let admin = PgPool::connect_with(postgres_test_database_options(&url, &database))
        .await
        .unwrap();
    apply_postgres_migrations_through_version(&admin, "0121")
        .await
        .unwrap();
    canonical_apparatus_service(admin.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    apply_foundation_migration(&admin).await.unwrap();
    sqlx::raw_sql("INSERT INTO mini_warehouses(id,name,parent_warehouse) VALUES ('names-w','Names warehouse','');
        INSERT INTO mini_warehouse_assignments(warehouse,warehouse_name,principal_role,principal_ref,assignment_kind)
        VALUES ('Names warehouse','Names warehouse','qolipchi','names-clerk','warehouse');")
        .execute(&admin).await.unwrap();
    let runtime = PgPool::connect_with(
        postgres_test_database_options(&url, &database).username("mini_rs_erp"),
    )
    .await
    .unwrap();
    let service = QolipService::new(Arc::new(PostgresQolipStore::new(runtime.clone())));
    let principal = Principal {
        role: PrincipalRole::Qolipchi,
        ref_: "names-clerk".into(),
        display_name: "Qolipchi".into(),
        legal_name: String::new(),
        phone: String::new(),
        avatar_url: String::new(),
    };
    let mut sets = Vec::new();
    for code in ["NAME-A", "NAME-B"] {
        let set = service
            .create_set(
                &QolipSetCreate {
                    warehouse: "Names warehouse".into(),
                    members: vec![QolipMoldDraft {
                        qolip_code: code.into(),
                        size: 420,
                        color: "BLACK".into(),
                        ..Default::default()
                    }],
                },
                &principal,
                false,
            )
            .await
            .unwrap();
        assert!(set.name.is_empty());
        sets.push(set);
    }
    let request = |index: usize, name: &str| QolipSetRename {
        set_id: sets[index].snapshot.set_id.clone(),
        name: name.into(),
    };
    let saved = service
        .rename_set(&request(0, "  Hot   lunch  "), &principal, false)
        .await
        .unwrap();
    assert_eq!(saved.name, "Hot lunch");
    assert_eq!(saved.snapshot.fingerprint, sets[0].snapshot.fingerprint);
    let resolved = service
        .resolve_shared_set("  hot   LUNCH ")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.set_id, sets[0].snapshot.set_id);
    assert_eq!(resolved.fingerprint, sets[0].snapshot.fingerprint);
    assert!(
        service
            .resolve_shared_set("Hot lun")
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        service
            .rename_set(&request(1, "HOT LUNCH"), &principal, false)
            .await,
        Err(QolipError::SetNameConflict)
    ));
    let a = request(0, "Concurrent");
    let b = request(1, "concurrent");
    let (a, b) = tokio::join!(
        service.rename_set(&a, &principal, false),
        service.rename_set(&b, &principal, false)
    );
    let results = [a, b];
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(QolipError::SetNameConflict)))
            .count(),
        1
    );

    service
        .send_reworks(
            QolipReworkSend {
                request_id: "away".into(),
                qolip_codes: vec!["NAME-A".into()],
            },
            &principal,
            false,
        )
        .await
        .unwrap();
    let renamed = service
        .rename_set(&request(0, "Away set"), &principal, false)
        .await
        .unwrap();
    assert_eq!(renamed.name, "Away set");
    assert_eq!(renamed.snapshot.members, sets[0].snapshot.members);
    let fresh = QolipService::new(Arc::new(PostgresQolipStore::new(runtime.clone())));
    let listed = fresh.managed_sets(&principal, false).await.unwrap();
    assert!(
        listed
            .iter()
            .any(|s| s.name == "Away set" && s.snapshot.set_id == sets[0].snapshot.set_id)
    );
    let mut outsider = principal.clone();
    outsider.ref_ = "another-clerk".into();
    assert!(matches!(
        fresh
            .rename_set(&request(0, "Forbidden"), &outsider, false)
            .await,
        Err(QolipError::SetChanged)
    ));
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT name FROM mini_qolip_set_names WHERE set_id=$1")
            .bind(&sets[0].snapshot.set_id)
            .fetch_one(&runtime)
            .await
            .unwrap(),
        "Away set"
    );

    runtime.close().await;
    admin.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database}\" WITH (FORCE)"))
        .execute(&server)
        .await
        .unwrap();
    server.close().await;
}
