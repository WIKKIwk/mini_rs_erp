mod customer_login;
mod helpers;
mod login;
mod runtime;
mod supplier_login;
mod worker_login;

use std::sync::Arc;
use std::sync::RwLock;

use crate::config::AppConfig;
use crate::core::auth::ports::{
    AdminAccessStateLookup, CustomerLookup, SupplierLookup, WorkerLookup,
};

pub use self::helpers::normalize_phone;
use self::helpers::{blank_default, normalize_config_phone};

#[derive(Clone)]
pub struct AuthService {
    supplier_prefix: String,
    werka_prefix: String,
    identity: Arc<RwLock<AuthIdentity>>,
    admin_code: String,
    supplier_lookup: Option<Arc<dyn SupplierLookup>>,
    customer_lookup: Option<Arc<dyn CustomerLookup>>,
    worker_lookup: Option<Arc<dyn WorkerLookup>>,
    admin_state_lookup: Option<Arc<dyn AdminAccessStateLookup>>,
}

#[derive(Debug, Clone)]
pub(super) struct AuthIdentity {
    werka_phone: String,
    werka_code: String,
    werka_name: String,
    admin_phone: String,
    admin_name: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("invalid role")]
    InvalidRole,
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
                werka_code: config.werka_code.trim().to_string(),
                werka_name: blank_default(&config.werka_name, "Werka"),
                admin_phone: normalize_config_phone(&config.admin_phone)
                    .unwrap_or_else(|_| config.admin_phone.trim().to_string()),
                admin_name: blank_default(&config.admin_name, "Admin"),
            })),
            admin_code: config.admin_code.trim().to_string(),
            supplier_lookup: None,
            customer_lookup: None,
            worker_lookup: None,
            admin_state_lookup: None,
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

    pub fn with_worker_dependencies(
        mut self,
        worker_lookup: Arc<dyn WorkerLookup>,
        admin_state_lookup: Arc<dyn AdminAccessStateLookup>,
    ) -> Self {
        self.worker_lookup = Some(worker_lookup);
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
}
