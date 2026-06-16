use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::models::PrincipalRole;
use super::ports::{
    AdminAccessState, AdminAccessStateLookup, AuthPortError, CustomerLookup, CustomerRecord,
    SupplierLookup, SupplierRecord, WorkerLookup, WorkerRecord,
};
use super::service::{AuthService, normalize_phone};
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
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
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
async fn supplier_login_uses_deterministic_code() {
    let suppliers = Arc::new(FakeSupplierLookup {
        suppliers: vec![SupplierRecord {
            id: "SUP-001".to_string(),
            name: "Abdulloh".to_string(),
            phone: "+998901234567".to_string(),
        }],
    });
    let states = Arc::new(FakeStateLookup::default());
    let auth = AuthService::new(&config()).with_supplier_dependencies(suppliers, states);

    let principal = auth
        .login("+998901234567", "104LJINSVVO5")
        .await
        .expect("supplier login");

    assert_eq!(principal.role, PrincipalRole::Supplier);
    assert_eq!(principal.ref_, "SUP-001");
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
        Ok(self.states.clone())
    }
}
