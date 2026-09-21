use std::collections::BTreeMap;
use std::sync::Arc;

use mini_rs_erp::config::{AppConfig, DotEnvPersister};
use mini_rs_erp::core::admin::models::AdminState;
use mini_rs_erp::core::admin::ports::{AdminReadPort, AdminStatePort, AdminWritePort};
use mini_rs_erp::core::admin::service::AdminService;
use mini_rs_erp::core::auth::models::PrincipalRole;
use mini_rs_erp::core::auth::password::{is_password_hash, verify_password};
use mini_rs_erp::core::auth::service::AuthService;
use mini_rs_erp::core::system_users::{MemorySystemUserStore, SystemUserService, SystemUserUpsert};
use mini_rs_erp::core::workers::{MemoryWorkerStore, WorkerService, WorkerUpsert};
use mini_rs_erp::db::postgres_auth::{LegacyBuiltinCredential, PostgresAuthStore};
use mini_rs_erp::store::admin_store::JsonAdminStore;
use sqlx::{PgPool, Row};

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database with the ERP owner/runtime roles"]
async fn postgres_credential_cutover_and_all_login_roles() {
    let url =
        std::env::var("MINI_ERP_AUTH_TEST_DATABASE_URL").expect("disposable test database URL");
    let pool = PgPool::connect(&url).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/postgres/0126_auth_credentials.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/postgres/0129_admin_access_code_visibility.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let store = Arc::new(PostgresAuthStore::new(pool.clone()));
    assert!(store.require_ready().await.is_err());
    let temp = tempfile::tempdir().unwrap();
    let json_path = temp.path().join("legacy.json");
    let legacy = Arc::new(JsonAdminStore::new(json_path.clone()));
    let supplier = legacy
        .create_supplier("Supplier", "+998901234501")
        .await
        .unwrap();
    let derived_supplier = legacy
        .create_supplier("Legacy derived", "+998901234511")
        .await
        .unwrap();
    let derived_code = mini_rs_erp::core::auth::access_codes::supplier_access_code(
        &mini_rs_erp::core::auth::access_codes::SupplierAccessInput {
            ref_: derived_supplier.ref_.clone(),
            name: derived_supplier.name.clone(),
            phone: derived_supplier.phone.clone(),
        },
    )
    .unwrap();
    let customer = legacy
        .create_customer("Customer", "+998901234502")
        .await
        .unwrap();
    let material = legacy
        .create_material_taminotchi("Material", "+998901234503")
        .await
        .unwrap();
    let workers = Arc::new(WorkerService::new(Arc::new(MemoryWorkerStore::new())));
    let worker = workers
        .upsert_worker(WorkerUpsert {
            id: "worker-auth".into(),
            name: "Worker".into(),
            phone: "+998901234504".into(),
            level: "Master".into(),
        })
        .await
        .unwrap();
    let system_users = Arc::new(SystemUserService::new(Arc::new(
        MemorySystemUserStore::new(),
    )));
    let mut states = BTreeMap::from([
        (
            supplier.ref_.clone(),
            AdminState {
                custom_code: "101234567890".into(),
                ..Default::default()
            },
        ),
        (
            customer.ref_.clone(),
            AdminState {
                custom_code: "301234567890".into(),
                ..Default::default()
            },
        ),
        (
            material.ref_.clone(),
            AdminState {
                custom_code: "701234567890".into(),
                ..Default::default()
            },
        ),
        (
            worker.id.clone(),
            AdminState {
                custom_code: "401234567890".into(),
                ..Default::default()
            },
        ),
    ]);
    let mut cases = vec![
        (
            "+998901234500".to_string(),
            "87654321".to_string(),
            PrincipalRole::Admin,
        ),
        (
            supplier.phone.clone(),
            "101234567890".into(),
            PrincipalRole::Supplier,
        ),
        (
            derived_supplier.phone.clone(),
            derived_code,
            PrincipalRole::Supplier,
        ),
        (
            customer.phone.clone(),
            "301234567890".into(),
            PrincipalRole::Customer,
        ),
        (
            material.phone.clone(),
            "701234567890".into(),
            PrincipalRole::MaterialTaminotchi,
        ),
        (
            worker.phone.clone(),
            "401234567890".into(),
            PrincipalRole::Aparatchi,
        ),
        (
            "+998901234509".into(),
            "201234567890".into(),
            PrincipalRole::Werka,
        ),
        (
            "+998901234510".into(),
            "601234567890".into(),
            PrincipalRole::MaterialTaminotchi,
        ),
    ];
    for (index, role, prefix) in [
        (5, PrincipalRole::Qolipchi, "50"),
        (6, PrincipalRole::Boyoqchi, "80"),
        (7, PrincipalRole::TayyorlovMasteri, "90"),
        (8, PrincipalRole::HomashyoRezkachi, "91"),
    ] {
        let phone = format!("+99890123450{index}");
        let user = system_users
            .upsert_user(SystemUserUpsert {
                id: format!("system-{index}"),
                role,
                name: format!("User {index}"),
                phone: phone.clone(),
            })
            .await
            .unwrap();
        let code = format!("{prefix}1234567890");
        states.insert(
            user.id,
            AdminState {
                custom_code: code.clone(),
                ..Default::default()
            },
        );
        cases.push((phone, code, role));
    }
    let mut legacy_file: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&json_path).unwrap()).unwrap();
    legacy_file["states"] = serde_json::to_value(&states).unwrap();
    std::fs::write(&json_path, serde_json::to_vec(&legacy_file).unwrap()).unwrap();
    let legacy = Arc::new(JsonAdminStore::new(json_path.clone()));
    let builtin = |id: &str, phone: &str, code: &str| LegacyBuiltinCredential {
        principal_ref: id.into(),
        phone: phone.into(),
        name: id.into(),
        code: code.into(),
    };
    // A missing administrator aborts the entire import, including supplier codes.
    assert!(
        store
            .migrate_legacy(states.clone(), vec![], vec![])
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mini_auth_accounts")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert!(
        store
            .migrate_legacy(
                legacy.states().await.unwrap(),
                legacy.suppliers_page("", 0, 0).await.unwrap(),
                vec![
                    builtin("admin", "+998901234500", "87654321"),
                    builtin("werka", "+998901234509", "201234567890"),
                    builtin("material_taminotchi", "+998901234510", "601234567890"),
                ]
            )
            .await
            .unwrap()
    );
    store.require_ready().await.unwrap();
    assert!(
        !store
            .migrate_legacy(BTreeMap::new(), vec![], vec![])
            .await
            .unwrap()
    );

    let config = AppConfig::from_env().unwrap();
    let auth = AuthService::new(&config)
        .with_supplier_dependencies(legacy.clone(), store.clone())
        .with_customer_dependencies(legacy.clone(), store.clone())
        .with_material_taminotchi_dependencies(legacy.clone(), store.clone())
        .with_worker_dependencies(workers, store.clone())
        .with_system_user_dependencies(system_users.clone(), store.clone());
    for (phone, code, role) in &cases {
        assert_eq!(auth.login(phone, code).await.unwrap().role, *role);
    }
    for row in sqlx::query("SELECT credential_hash, access_state FROM mini_auth_accounts")
        .fetch_all(&pool)
        .await
        .unwrap()
    {
        let hash: String = row.get("credential_hash");
        let state: serde_json::Value = row.get("access_state");
        assert!(is_password_hash(&hash));
        assert!(state.get("custom_code").is_none());
        assert!(state.get("pending_persist_code").is_none());
        for (_, code, _) in &cases {
            assert!(!hash.contains(code));
        }
    }
    assert!(sqlx::query("UPDATE mini_auth_accounts SET credential_hash = 'plaintext' WHERE principal_ref = 'admin'")
        .execute(&pool).await.is_err());
    assert!(sqlx::query("UPDATE mini_auth_accounts SET access_state = '{\"custom_code\":\"secret\"}' WHERE principal_ref = 'admin'")
        .execute(&pool).await.is_err());
    let can_delete: bool = sqlx::query_scalar(
        "SELECT has_table_privilege('mini_rs_erp', 'mini_auth_accounts', 'DELETE')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!can_delete);

    let admin = AdminService::new(&config)
        .with_state_port(store.clone())
        .with_read_port(legacy.clone())
        .with_write_port(legacy.clone());
    assert_eq!(
        admin.supplier_detail(&supplier.ref_).await.unwrap().code,
        "101234567890"
    );
    assert_eq!(
        admin.customer_detail(&customer.ref_).await.unwrap().code,
        "301234567890"
    );
    assert_eq!(
        admin.worker_detail(worker.clone()).await.unwrap().code,
        "401234567890"
    );
    assert_eq!(admin.settings().await.unwrap().werka_code, "201234567890");
    let supplier_rotated = admin
        .regenerate_supplier_code(&supplier.ref_)
        .await
        .unwrap();
    assert!(auth.login(&supplier.phone, "101234567890").await.is_err());
    assert!(
        auth.login(&supplier.phone, &supplier_rotated.code)
            .await
            .is_ok()
    );
    assert_eq!(
        admin.supplier_detail(&supplier.ref_).await.unwrap().code,
        supplier_rotated.code
    );
    let material_rotated = admin
        .regenerate_material_taminotchi_code(&material.ref_)
        .await
        .unwrap();
    assert!(auth.login(&material.phone, "701234567890").await.is_err());
    assert!(
        auth.login(&material.phone, &material_rotated.code)
            .await
            .is_ok()
    );
    assert_eq!(
        admin
            .material_taminotchi_detail(&material.ref_)
            .await
            .unwrap()
            .code,
        material_rotated.code
    );
    for (phone, old_code, role) in &cases {
        if matches!(
            role,
            PrincipalRole::Qolipchi
                | PrincipalRole::Boyoqchi
                | PrincipalRole::TayyorlovMasteri
                | PrincipalRole::HomashyoRezkachi
        ) {
            let principal = auth.login(phone, old_code).await.unwrap();
            let user = system_users
                .users_by_ids(&[principal.ref_])
                .await
                .unwrap()
                .pop()
                .unwrap();
            let rotated = admin
                .regenerate_system_user_code(user.clone())
                .await
                .unwrap();
            assert!(auth.login(phone, old_code).await.is_err());
            assert!(auth.login(phone, &rotated.code).await.is_ok());
            assert_eq!(
                admin.system_user_detail(user).await.unwrap().code,
                rotated.code
            );
        }
    }
    let rotated = admin
        .regenerate_customer_code(&customer.ref_)
        .await
        .unwrap();
    assert!(!rotated.code.is_empty());
    assert_eq!(
        admin.customer_detail(&customer.ref_).await.unwrap().code,
        rotated.code
    );
    assert!(auth.login(&customer.phone, "301234567890").await.is_err());
    assert_eq!(
        auth.login(&customer.phone, &rotated.code)
            .await
            .unwrap()
            .role,
        PrincipalRole::Customer
    );
    let worker_rotated = admin.regenerate_worker_code(worker.clone()).await.unwrap();
    assert!(auth.login(&worker.phone, "401234567890").await.is_err());
    assert!(
        auth.login(&worker.phone, &worker_rotated.code)
            .await
            .is_ok()
    );
    let warehouse = admin.regenerate_werka_code().await.unwrap();
    assert_eq!(
        admin.settings().await.unwrap().werka_code,
        warehouse.werka_code
    );
    assert!(
        auth.login("+998901234509", &warehouse.werka_code)
            .await
            .is_ok()
    );

    let mut stale = store.states().await.unwrap()[&customer.ref_].clone();
    let rotated_again = admin
        .regenerate_customer_code(&customer.ref_)
        .await
        .unwrap();
    stale.blocked = true;
    store.put_state(&customer.ref_, stale).await.unwrap();
    let current = store.states().await.unwrap()[&customer.ref_].clone();
    assert!(
        verify_password(&current.custom_code, &rotated_again.code)
            .await
            .unwrap()
    );
    assert!(
        auth.login(&customer.phone, &rotated_again.code)
            .await
            .is_err()
    );

    store
        .reset_admin_code("admin-replacement-credential".into())
        .await
        .unwrap();
    assert!(auth.login("+998901234500", "87654321").await.is_err());
    assert!(
        auth.login("+998901234500", "admin-replacement-credential")
            .await
            .is_ok()
    );
    // Reopening stores models a restart: no fallback to old environment/JSON codes.
    let reopened = PostgresAuthStore::new(pool.clone());
    reopened.require_ready().await.unwrap();
    assert!(
        verify_password(
            &reopened.states().await.unwrap()["admin"].custom_code,
            "admin-replacement-credential"
        )
        .await
        .unwrap()
    );

    // Reopening must keep the same readable code and must not change login hashes.
    assert_eq!(
        reopened.access_code(&worker.id).await.unwrap(),
        worker_rotated.code
    );
    assert_eq!(
        reopened.access_code(&supplier.ref_).await.unwrap(),
        supplier_rotated.code
    );
    assert_eq!(
        reopened.access_code(&customer.ref_).await.unwrap(),
        rotated_again.code
    );
    assert_eq!(
        reopened.access_code("werka").await.unwrap(),
        warehouse.werka_code
    );
    assert_eq!(
        reopened.access_code("admin").await.unwrap(),
        "admin-replacement-credential"
    );
    let reopened_admin = AdminService::new(&config)
        .with_state_port(Arc::new(reopened))
        .with_read_port(legacy.clone())
        .with_write_port(legacy.clone());
    assert_eq!(
        reopened_admin
            .worker_detail(worker.clone())
            .await
            .unwrap()
            .code,
        worker_rotated.code
    );
    // Legacy hash-only accounts recover after a normal successful login, with no reset.
    sqlx::query("DELETE FROM mini_auth_code_vault WHERE principal_ref = $1")
        .bind(&worker.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(store.access_code(&worker.id).await.unwrap().is_empty());
    let before_hash = store.states().await.unwrap()[&worker.id]
        .custom_code
        .clone();
    assert!(
        !store
            .recover_access_code(&worker.id, "401111111111")
            .await
            .unwrap()
    );
    assert!(
        auth.login(&worker.phone, &worker_rotated.code)
            .await
            .is_ok()
    );
    assert_eq!(
        store.access_code(&worker.id).await.unwrap(),
        worker_rotated.code
    );
    assert_eq!(
        store.states().await.unwrap()[&worker.id].custom_code,
        before_hash
    );
    // Admin regeneration has no cooldown, including a pre-existing cooldown value.
    let mut state = store.states().await.unwrap()[&worker.id].clone();
    state.cooldown_until = Some(time::OffsetDateTime::now_utc() + time::Duration::hours(1));
    store.put_state(&worker.id, state).await.unwrap();
    for _ in 0..5 {
        let issued = admin.regenerate_worker_code(worker.clone()).await.unwrap();
        assert!(!issued.code_locked);
        assert_eq!(issued.code_retry_after_sec, 0);
        assert_eq!(store.access_code(&worker.id).await.unwrap(), issued.code);
    }
    for row in sqlx::query("SELECT encrypted_code FROM mini_auth_code_vault")
        .fetch_all(&pool)
        .await
        .unwrap()
    {
        let ciphertext: String = row.get("encrypted_code");
        assert!(ciphertext.starts_with("v1:"));
        for (_, code, _) in &cases {
            assert!(!ciphertext.contains(code));
        }
    }

    legacy.clear_legacy_access_secrets().await.unwrap();
    let stored = std::fs::read_to_string(&json_path).unwrap();
    for (_, code, _) in &cases {
        assert!(!stored.contains(code));
    }
    let env_path = temp.path().join(".env");
    std::fs::write(
        &env_path,
        "ADMINKA_CODE=87654321\nMOBILE_DEV_WERKA_CODE=201234567890\nKEEP_ME=preserved\n",
    )
    .unwrap();
    DotEnvPersister::new(&env_path)
        .remove_keys(&["ADMINKA_CODE", "MOBILE_DEV_WERKA_CODE"])
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(&env_path).unwrap(),
        "KEEP_ME=preserved\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&env_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&json_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    pool.close().await;
}
