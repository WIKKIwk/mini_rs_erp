mod customer_login;
mod helpers;
mod login;
mod material_taminotchi_login;
mod runtime;
mod supplier_login;
mod throttle;
mod worker_login;

use std::sync::Arc;
use std::sync::RwLock;

use crate::config::AppConfig;
use crate::core::auth::ports::{
    AdminAccessStateLookup, CustomerLookup, MaterialTaminotchiLookup, SupplierLookup,
    SystemUserLookup, WorkerLookup,
};

pub use self::helpers::normalize_phone;
use self::helpers::{blank_default, normalize_config_phone};
use crate::core::auth::password::{hash_password_sync, is_password_hash};

fn config_code_hash(code: &str) -> String {
    let code = code.trim();
    if code.is_empty() || is_password_hash(code) { return code.to_string(); }
    hash_password_sync(code).unwrap_or_default()
}

#[derive(Clone)]
pub struct AuthService {
    supplier_prefix: String,
    werka_prefix: String,
    identity: Arc<RwLock<AuthIdentity>>,
    admin_code: String,
    login_throttle: Arc<std::sync::Mutex<throttle::LoginThrottle>>,
    supplier_lookup: Option<Arc<dyn SupplierLookup>>,
    customer_lookup: Option<Arc<dyn CustomerLookup>>,
    material_taminotchi_lookup: Option<Arc<dyn MaterialTaminotchiLookup>>,
    worker_lookup: Option<Arc<dyn WorkerLookup>>,
    system_user_lookup: Option<Arc<dyn SystemUserLookup>>,
    admin_state_lookup: Option<Arc<dyn AdminAccessStateLookup>>,
    builtin_credentials_from_store: bool,
}

#[derive(Debug, Clone)]
pub(super) struct AuthIdentity {
    werka_phone: String,
    werka_code: String,
    werka_name: String,
    material_taminotchi_phone: String,
    material_taminotchi_code: String,
    material_taminotchi_name: String,
    admin_phone: String,
    admin_name: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("invalid role")]
    InvalidRole,
    #[error("too many login attempts; try again later")]
    TooManyAttempts,
    #[error("internal auth error")]
    Internal,
}

impl AuthService {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            supplier_prefix: blank_default(&config.supplier_prefix, "10"),
            werka_prefix: blank_default(&config.werka_prefix, "20"),
            identity: Arc::new(RwLock::new(AuthIdentity {
                werka_phone: normalize_config_phone(&config.werka_phone)
                    .unwrap_or_else(|_| config.werka_phone.trim().to_string()),
                werka_code: config_code_hash(&config.werka_code),
                werka_name: blank_default(&config.werka_name, "Werka"),
                material_taminotchi_phone: normalize_config_phone(
                    &config.material_taminotchi_phone,
                )
                .unwrap_or_else(|_| config.material_taminotchi_phone.trim().to_string()),
                material_taminotchi_code: config_code_hash(&config.material_taminotchi_code),
                material_taminotchi_name: blank_default(
                    &config.material_taminotchi_name,
                    "Material taminotchisi",
                ),
                admin_phone: normalize_config_phone(&config.admin_phone)
                    .unwrap_or_else(|_| config.admin_phone.trim().to_string()),
                admin_name: blank_default(&config.admin_name, "Admin"),
            })),
            admin_code: config_code_hash(&config.admin_code),
            login_throttle: Arc::new(std::sync::Mutex::new(throttle::LoginThrottle::default())),
            supplier_lookup: None,
            customer_lookup: None,
            material_taminotchi_lookup: None,
            worker_lookup: None,
            system_user_lookup: None,
            admin_state_lookup: None,
            builtin_credentials_from_store: false,
        }
    }

    pub fn with_supplier_dependencies(
        mut self,
        supplier_lookup: Arc<dyn SupplierLookup>,
        admin_state_lookup: Arc<dyn AdminAccessStateLookup>,
    ) -> Self {
        self.supplier_lookup = Some(supplier_lookup);
        self.admin_state_lookup = Some(admin_state_lookup);
        self
    }

    pub fn with_access_state_lookup(mut self, lookup: Arc<dyn AdminAccessStateLookup>) -> Self {
        self.admin_state_lookup = Some(lookup);
        self.builtin_credentials_from_store = true;
        self
    }

    pub fn with_worker_dependencies(
        mut self,
        worker_lookup: Arc<dyn WorkerLookup>,
        admin_state_lookup: Arc<dyn AdminAccessStateLookup>,
    ) -> Self {
        self.worker_lookup = Some(worker_lookup);
        self.admin_state_lookup = Some(admin_state_lookup);
        self
    }

    pub fn with_system_user_dependencies(
        mut self,
        system_user_lookup: Arc<dyn SystemUserLookup>,
        admin_state_lookup: Arc<dyn AdminAccessStateLookup>,
    ) -> Self {
        self.system_user_lookup = Some(system_user_lookup);
        self.admin_state_lookup = Some(admin_state_lookup);
        self
    }

    pub fn with_customer_dependencies(
        mut self,
        customer_lookup: Arc<dyn CustomerLookup>,
        admin_state_lookup: Arc<dyn AdminAccessStateLookup>,
    ) -> Self {
        self.customer_lookup = Some(customer_lookup);
        self.admin_state_lookup = Some(admin_state_lookup);
        self
    }

    pub fn with_material_taminotchi_dependencies(
        mut self,
        material_taminotchi_lookup: Arc<dyn MaterialTaminotchiLookup>,
        admin_state_lookup: Arc<dyn AdminAccessStateLookup>,
    ) -> Self {
        self.material_taminotchi_lookup = Some(material_taminotchi_lookup);
        self.admin_state_lookup = Some(admin_state_lookup);
        self
    }
}
