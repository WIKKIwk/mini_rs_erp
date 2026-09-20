use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::models::PrincipalRole;
use super::ports::{
    AdminAccessState, AdminAccessStateLookup, AuthPortError, CustomerLookup, CustomerRecord,
    SupplierLookup, SupplierRecord, WorkerLookup, WorkerRecord,
};
use super::service::{AuthError, AuthService, normalize_phone};
use crate::config::AppConfig;

fn config() -> AppConfig {
    AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: String::new(),
        http_timeout: std::time::Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: "data/mobile_profile_prefs.json".into(),
        push_token_store_path: "data/mobile_push_tokens.json".into(),
        session_ttl_seconds: Some(30 * 24 * 60 * 60),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: "20ABCDEF1234".to_string(),
        werka_name: "Werka".to_string(),
        werka_phone: "+998888862440".to_string(),
        material_taminotchi_code: "60ABCDEF1234".to_string(),
        material_taminotchi_name: "Material taminotchisi".to_string(),
        material_taminotchi_phone: "+998901006060".to_string(),
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
    }
}

#[tokio::test]
async fn raw_material_split_role_login_is_independent_and_blocked_users_are_denied() {
    use crate::core::system_users::{SystemUserService,MemorySystemUserStore,SystemUserUpsert};
    let users=Arc::new(SystemUserService::new(Arc::new(MemorySystemUserStore::new())));
    let user=users.upsert_user(SystemUserUpsert { id:"raw-cutter".into(),role:PrincipalRole::HomashyoRezkachi,
        name:"Cutter".into(),phone:"+998901112291".into() }).await.unwrap();
    assert_eq!(users.users(&PrincipalRole::HomashyoRezkachi,"",10).await.unwrap(),vec![user]);
    assert!(users.users(&PrincipalRole::Qolipchi,"",10).await.unwrap().is_empty());
    for blocked in [false,true] {
        let states=Arc::new(FakeStateLookup { states:BTreeMap::from([("raw-cutter".into(),
            AdminAccessState {custom_code:"911234567890".into(),blocked,removed:false})]) });
        let auth=AuthService::new(&config()).with_system_user_dependencies(users.clone(),states);
        let result=auth.login("+998901112291","911234567890").await;
        if blocked {assert_eq!(result.unwrap_err(),AuthError::InvalidCredentials);}
        else {let principal=result.unwrap();assert_eq!(principal.role,PrincipalRole::HomashyoRezkachi);assert_eq!(principal.ref_,"raw-cutter");}
        for code in ["91ABCDEF1234","401234567890","901234567890"] {
            assert!(auth.login("+998901112291",code).await.is_err());
        }
    }
}

#[test]
fn normalizes_phone_like_go() {
    assert_eq!(normalize_phone("888862440").unwrap(), "+998888862440");
    assert_eq!(normalize_phone("998901234567").unwrap(), "+998901234567");
    assert!(normalize_phone("+12345").is_err());
    assert!(normalize_phone("+998 90").is_err());
}

#[tokio::test]
async fn admin_login_does_not_need_erp() {
    let auth = AuthService::new(&config());
    let principal = auth
        .login("+998880000000", "19621978")
        .await
        .expect("admin login");

    assert_eq!(principal.role, PrincipalRole::Admin);
    assert_eq!(principal.ref_, "admin");
}

#[tokio::test]
async fn login_limit_is_shared_by_phone_variants_and_service_clones() {
    let config = config();
    let auth = AuthService::new(&config);
    for phone in [
        "880000000",
        "+998880000000",
        "998880000000",
        " 880000000 ",
        "880000000",
    ] {
        assert!(auth.clone().login(phone, "wrong-code").await.is_err());
    }
    assert_eq!(
        auth.login(&config.admin_phone, &config.admin_code).await,
        Err(AuthError::TooManyAttempts)
    );
    assert_ne!(
        auth.login("+998901234567", "wrong-code").await,
        Err(AuthError::TooManyAttempts)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_successful_logins_are_not_limited() {
    let config = config();
    let auth = AuthService::new(&config);
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..20 {
        let auth = auth.clone();
        let config = config.clone();
        tasks.spawn(async move { auth.login(&config.admin_phone, &config.admin_code).await });
    }
    let mut accepted = 0;
    while let Some(result) = tasks.join_next().await {
        result.expect("login task").expect("successful login");
        accepted += 1;
    }
    assert_eq!(accepted, 20);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_failed_logins_cannot_bypass_the_phone_limit() {
    let config = config();
    let auth = AuthService::new(&config);
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..20 {
        let auth = auth.clone();
        let phone = config.admin_phone.clone();
        tasks.spawn(async move { auth.login(&phone, "wrong-code").await });
    }
    let mut rejected_credentials = 0;
    let mut throttled = 0;
    while let Some(result) = tasks.join_next().await {
        match result.expect("login task") {
            Err(AuthError::InvalidCredentials | AuthError::InvalidRole) => {
                rejected_credentials += 1
            }
            Err(AuthError::TooManyAttempts) => throttled += 1,
            result => panic!("unexpected authentication result: {result:?}"),
        }
    }
    assert_eq!(rejected_credentials, 5);
    assert_eq!(throttled, 15);
}

#[tokio::test]
async fn successful_login_clears_previous_failures() {
    let config = config();
    let auth = AuthService::new(&config);
    for _ in 0..10 {
        for _ in 0..4 {
            assert!(matches!(
                auth.login(&config.admin_phone, "wrong-code").await,
                Err(AuthError::InvalidCredentials | AuthError::InvalidRole)
            ));
        }
        assert_eq!(
            auth.login(&config.admin_phone, &config.admin_code)
                .await
                .unwrap()
                .role,
            PrincipalRole::Admin
        );
    }
}

#[tokio::test]
async fn admin_login_requires_the_configured_credentials() {
    let original = config();
    let mut changed = original.clone();
    changed.admin_phone = "+998901234567".to_string();
    changed.admin_code = "7294810365827406".to_string();
    let auth = AuthService::new(&changed);
    assert!(
        auth.login(&original.admin_phone, &original.admin_code)
            .await
            .is_err()
    );
    assert!(
        auth.login(&changed.admin_phone, &original.admin_code)
            .await
            .is_err()
    );
    assert_eq!(
        auth.login(&changed.admin_phone, &changed.admin_code)
            .await
            .unwrap()
            .role,
        PrincipalRole::Admin
    );
    changed.admin_code.clear();
    assert!(
        AuthService::new(&changed)
            .login(&changed.admin_phone, "")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn werka_login_requires_configured_phone() {
    let auth = AuthService::new(&config());
    let principal = auth
        .login("+998888862440", "20ABCDEF1234")
        .await
        .expect("werka login");

    assert_eq!(principal.role, PrincipalRole::Werka);
    assert_eq!(principal.ref_, "werka");
    let local_phone_principal = auth
        .login("888862440", "20ABCDEF1234")
        .await
        .expect("werka login with local phone");
    assert_eq!(local_phone_principal.role, PrincipalRole::Werka);
    assert!(auth.login("+998880000000", "20ABCDEF1234").await.is_err());
}

#[tokio::test]
async fn material_taminotchi_login_uses_sixty_prefix_and_configured_identity() {
    let auth = AuthService::new(&config());
    let principal = auth
        .login("+998901006060", "60ABCDEF1234")
        .await
        .expect("material taminotchi login");

    assert_eq!(principal.role, PrincipalRole::MaterialTaminotchi);
    assert_eq!(principal.ref_, "material_taminotchi");
    assert_eq!(principal.display_name, "Material taminotchisi");
    let local_phone_principal = auth
        .login("901006060", "60ABCDEF1234")
        .await
        .expect("material taminotchi login with local phone");
    assert_eq!(
        local_phone_principal.role,
        PrincipalRole::MaterialTaminotchi
    );
    assert!(auth.login("+998901006060", "50ABCDEF1234").await.is_err());
}

#[tokio::test]
async fn material_taminotchi_login_accepts_customer_custom_code() {
    let customers = Arc::new(FakeCustomerLookup {
        customers: vec![CustomerRecord {
            id: "CUST-MATERIAL".to_string(),
            name: "Materialchi".to_string(),
            phone: "+998901006060".to_string(),
        }],
    });
    let states = Arc::new(FakeStateLookup {
        states: BTreeMap::from([(
            "CUST-MATERIAL".to_string(),
            AdminAccessState {
                custom_code: "601122334455".to_string(),
                blocked: false,
                removed: false,
            },
        )]),
    });
    let auth = AuthService::new(&config()).with_customer_dependencies(customers, states);

    let principal = auth
        .login("+998901006060", "601122334455")
        .await
        .expect("material taminotchi customer login");

    assert_eq!(principal.role, PrincipalRole::MaterialTaminotchi);
    assert_eq!(principal.ref_, "CUST-MATERIAL");
    assert_eq!(principal.display_name, "Materialchi");
}

#[tokio::test]
async fn supplier_login_accepts_migrated_deterministic_code() {
    let suppliers = Arc::new(FakeSupplierLookup {
        suppliers: vec![SupplierRecord {
            id: "SUP-001".to_string(),
            name: "Abdulloh".to_string(),
            phone: "+998901234567".to_string(),
        }],
    });
    let states = Arc::new(FakeStateLookup {
        states: BTreeMap::from([("SUP-001".to_string(), AdminAccessState {
            custom_code: "104LJINSVVO5".into(), ..Default::default()
        })]),
    });
    let auth = AuthService::new(&config()).with_supplier_dependencies(suppliers, states);

    let principal = auth
        .login("+998901234567", "104LJINSVVO5")
        .await
        .expect("supplier login");

    assert_eq!(principal.role, PrincipalRole::Supplier);
    assert_eq!(principal.ref_, "SUP-001");
}

#[tokio::test]
async fn database_mode_rejects_missing_builtin_without_config_fallback() {
    let config = config();
    let auth = AuthService::new(&config).with_access_state_lookup(Arc::new(FakeStateLookup::default()));
    assert!(auth.login(&config.admin_phone, &config.admin_code).await.is_err());
    assert!(auth.login(&config.werka_phone, &config.werka_code).await.is_err());
}

#[tokio::test]
async fn supplier_login_never_derives_a_code_without_a_stored_hash() {
    let suppliers = Arc::new(FakeSupplierLookup {
        suppliers: vec![SupplierRecord {
            id: "SUP-001".into(), name: "Abdulloh".into(), phone: "+998901234567".into(),
        }],
    });
    let states = Arc::new(FakeStateLookup {
        states: BTreeMap::from([("SUP-001".into(), AdminAccessState::default())]),
    });
    let auth = AuthService::new(&config()).with_supplier_dependencies(suppliers, states);
    assert_eq!(auth.login("+998901234567", "104LJINSVVO5").await, Err(AuthError::InvalidCredentials));
}

#[tokio::test]
async fn supplier_login_rejects_missing_access_state_as_internal_error() {
    let suppliers = Arc::new(FakeSupplierLookup {
        suppliers: vec![SupplierRecord {
            id: "SUP-MISSING-STATE".to_string(),
            name: "Missing state".to_string(),
            phone: "+998901234567".to_string(),
        }],
    });
    let auth = AuthService::new(&config())
        .with_supplier_dependencies(suppliers, Arc::new(FakeStateLookup::default()));

    for _ in 0..10 {
        assert_eq!(
            auth.login("+998901234567", "104LJINSVVO5").await,
            Err(AuthError::Internal)
        );
    }
}

#[tokio::test]
async fn supplier_login_respects_custom_code_and_blocked_state() {
    let suppliers = Arc::new(FakeSupplierLookup {
        suppliers: vec![
            SupplierRecord {
                id: "SUP-BLOCKED".to_string(),
                name: "Blocked".to_string(),
                phone: "+998901234567".to_string(),
            },
            SupplierRecord {
                id: "SUP-OK".to_string(),
                name: "Open".to_string(),
                phone: "+998901234567".to_string(),
            },
        ],
    });
    let states = Arc::new(FakeStateLookup {
        states: BTreeMap::from([
            (
                "SUP-BLOCKED".to_string(),
                AdminAccessState {
                    custom_code: "10CUSTOM".to_string(),
                    blocked: true,
                    removed: false,
                },
            ),
            (
                "SUP-OK".to_string(),
                AdminAccessState {
                    custom_code: "10CUSTOM".to_string(),
                    blocked: false,
                    removed: false,
                },
            ),
        ]),
    });
    let auth = AuthService::new(&config()).with_supplier_dependencies(suppliers, states);

    let principal = auth
        .login("+998901234567", "10CUSTOM")
        .await
        .expect("supplier login");

    assert_eq!(principal.ref_, "SUP-OK");
}

#[tokio::test]
async fn supplier_login_accepts_local_phone() {
    let suppliers = Arc::new(FakeSupplierLookup {
        suppliers: vec![SupplierRecord {
            id: "SUP-LOCAL".to_string(),
            name: "Local Supplier".to_string(),
            phone: "901234567".to_string(),
        }],
    });
    let states = Arc::new(FakeStateLookup {
        states: BTreeMap::from([(
            "SUP-LOCAL".to_string(),
            AdminAccessState {
                custom_code: "10LOCAL".to_string(),
                blocked: false,
                removed: false,
            },
        )]),
    });
    let auth = AuthService::new(&config()).with_supplier_dependencies(suppliers, states);

    let principal = auth
        .login("901234567", "10LOCAL")
        .await
        .expect("supplier login");

    assert_eq!(principal.role, PrincipalRole::Supplier);
    assert_eq!(principal.ref_, "SUP-LOCAL");
}

#[tokio::test]
async fn customer_login_requires_custom_code() {
    let customers = Arc::new(FakeCustomerLookup {
        customers: vec![CustomerRecord {
            id: "CUST-001".to_string(),
            name: "Comfi".to_string(),
            phone: "+998901234567".to_string(),
        }],
    });
    let states = Arc::new(FakeStateLookup {
        states: BTreeMap::from([(
            "CUST-001".to_string(),
            AdminAccessState {
                custom_code: "30CUSTOM".to_string(),
                blocked: false,
                removed: false,
            },
        )]),
    });
    let auth = AuthService::new(&config()).with_customer_dependencies(customers, states);

    let principal = auth
        .login("+998901234567", "30CUSTOM")
        .await
        .expect("customer login");

    assert_eq!(principal.role, PrincipalRole::Customer);
    assert_eq!(principal.ref_, "CUST-001");
}

#[tokio::test]
async fn customer_login_accepts_local_phone() {
    let customers = Arc::new(FakeCustomerLookup {
        customers: vec![CustomerRecord {
            id: "CUST-LOCAL".to_string(),
            name: "Local Customer".to_string(),
            phone: "990000088".to_string(),
        }],
    });
    let states = Arc::new(FakeStateLookup {
        states: BTreeMap::from([(
            "CUST-LOCAL".to_string(),
            AdminAccessState {
                custom_code: "30LOCAL".to_string(),
                blocked: false,
                removed: false,
            },
        )]),
    });
    let auth = AuthService::new(&config()).with_customer_dependencies(customers, states);

    let principal = auth
        .login("990000088", "30LOCAL")
        .await
        .expect("customer login");

    assert_eq!(principal.role, PrincipalRole::Customer);
    assert_eq!(principal.ref_, "CUST-LOCAL");
}

#[tokio::test]
async fn aparatchi_login_uses_forty_prefix() {
    let customers = Arc::new(FakeCustomerLookup {
        customers: vec![CustomerRecord {
            id: "aparatchi - 4".to_string(),
            name: "aparatchi".to_string(),
            phone: "110000011".to_string(),
        }],
    });
    let states = Arc::new(FakeStateLookup {
        states: BTreeMap::from([(
            "aparatchi - 4".to_string(),
            AdminAccessState {
                custom_code: "401122334455".to_string(),
                blocked: false,
                removed: false,
            },
        )]),
    });
    let auth = AuthService::new(&config()).with_customer_dependencies(customers, states);

    let principal = auth
        .login("110000011", "401122334455")
        .await
        .expect("aparatchi login");

    assert_eq!(principal.role, PrincipalRole::Aparatchi);
    assert_eq!(principal.ref_, "aparatchi - 4");
}

#[tokio::test]
async fn aparatchi_login_accepts_worker_phone_and_code() {
    let workers = Arc::new(FakeWorkerLookup {
        workers: vec![WorkerRecord {
            id: "worker_001".to_string(),
            name: "Ali worker".to_string(),
            phone: "+998901112233".to_string(),
        }],
    });
    let states = Arc::new(FakeStateLookup {
        states: BTreeMap::from([(
            "worker_001".to_string(),
            AdminAccessState {
                custom_code: "401234567890".to_string(),
                blocked: false,
                removed: false,
            },
        )]),
    });
    let auth = AuthService::new(&config()).with_worker_dependencies(workers, states);

    let principal = auth
        .login("+998901112233", "401234567890")
        .await
        .expect("worker aparatchi login");

    assert_eq!(principal.role, PrincipalRole::Aparatchi);
    assert_eq!(principal.ref_, "worker_001");
    assert_eq!(principal.display_name, "Ali worker");
}

#[tokio::test]
async fn worker_and_system_user_login_rejects_non_numeric_codes() {
    let auth = AuthService::new(&config());

    for (phone, code) in [
        ("+998901112233", "40ABCDEF1234"),
        ("+998901112234", "50ABCDEF1234"),
        ("+998901112235", "80ABCDEF1234"),
    ] {
        assert!(auth.login(phone, code).await.is_err(), "code: {code}");
    }
}

#[tokio::test]
async fn customer_login_merges_local_phone_when_normalized_search_returns_other_matches() {
    let customers = Arc::new(FakeCustomerLookup {
        customers: vec![
            CustomerRecord {
                id: "aparatchi duplicate deploy check".to_string(),
                name: "duplicate".to_string(),
                phone: "+998110000011".to_string(),
            },
            CustomerRecord {
                id: "aparatchi - 4".to_string(),
                name: "aparatchi".to_string(),
                phone: "110000011".to_string(),
            },
        ],
    });
    let states = Arc::new(FakeStateLookup {
        states: BTreeMap::from([(
            "aparatchi - 4".to_string(),
            AdminAccessState {
                custom_code: "401122334455".to_string(),
                blocked: false,
                removed: false,
            },
        )]),
    });
    let auth = AuthService::new(&config()).with_customer_dependencies(customers, states);

    let principal = auth
        .login("110000011", "401122334455")
        .await
        .expect("aparatchi login");

    assert_eq!(principal.role, PrincipalRole::Aparatchi);
    assert_eq!(principal.ref_, "aparatchi - 4");
}

#[tokio::test]
async fn customer_login_fails_without_custom_code() {
    let customers = Arc::new(FakeCustomerLookup {
        customers: vec![CustomerRecord {
            id: "CUST-001".to_string(),
            name: "Comfi".to_string(),
            phone: "+998901234567".to_string(),
        }],
    });
    let states = Arc::new(FakeStateLookup::default());
    let auth = AuthService::new(&config()).with_customer_dependencies(customers, states);

    assert!(auth.login("+998901234567", "30CUSTOM").await.is_err());
}

struct FakeSupplierLookup {
    suppliers: Vec<SupplierRecord>,
}

#[async_trait]
impl SupplierLookup for FakeSupplierLookup {
    async fn search_suppliers(
        &self,
        query: &str,
        _limit: usize,
    ) -> Result<Vec<SupplierRecord>, AuthPortError> {
        Ok(self
            .suppliers
            .iter()
            .filter(|record| supplier_matches_query(record, query))
            .cloned()
            .collect())
    }
}

struct FakeCustomerLookup {
    customers: Vec<CustomerRecord>,
}

#[async_trait]
impl CustomerLookup for FakeCustomerLookup {
    async fn search_customers(
        &self,
        query: &str,
        _limit: usize,
    ) -> Result<Vec<CustomerRecord>, AuthPortError> {
        Ok(self
            .customers
            .iter()
            .filter(|record| customer_matches_query(record, query))
            .cloned()
            .collect())
    }
}

struct FakeWorkerLookup {
    workers: Vec<WorkerRecord>,
}

#[async_trait]
impl WorkerLookup for FakeWorkerLookup {
    async fn search_workers(
        &self,
        query: &str,
        _limit: usize,
    ) -> Result<Vec<WorkerRecord>, AuthPortError> {
        Ok(self
            .workers
            .iter()
            .filter(|record| worker_matches_query(record, query))
            .cloned()
            .collect())
    }
}

fn supplier_matches_query(record: &SupplierRecord, query: &str) -> bool {
    query_matches_fields(query, [&record.id, &record.name, &record.phone])
}

fn customer_matches_query(record: &CustomerRecord, query: &str) -> bool {
    query_matches_fields(query, [&record.id, &record.name, &record.phone])
}

fn worker_matches_query(record: &WorkerRecord, query: &str) -> bool {
    query_matches_fields(query, [&record.id, &record.name, &record.phone])
}

fn query_matches_fields<const N: usize>(query: &str, fields: [&String; N]) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || fields
            .iter()
            .any(|field| field.to_lowercase().contains(&query))
}

#[derive(Default)]
struct FakeStateLookup {
    states: BTreeMap<String, AdminAccessState>,
}

#[async_trait]
impl AdminAccessStateLookup for FakeStateLookup {
    async fn list_states(&self) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError> {
        let mut states = self.states.clone();
        for state in states.values_mut() {
            if !state.custom_code.is_empty() && !crate::core::auth::password::is_password_hash(&state.custom_code) {
                state.custom_code = crate::core::auth::password::hash_password(state.custom_code.clone())
                    .await.map_err(|_| AuthPortError::LookupFailed)?;
            }
        }
        Ok(states)
    }
}
