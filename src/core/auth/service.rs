use std::sync::Arc;
use std::sync::RwLock;

use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::auth::ports::{
    AdminAccessStateLookup, AuthConfigSink, CustomerLookup, CustomerRecord, SupplierLookup,
    WorkerLookup, WorkerRecord,
};

mod helpers;

pub use self::helpers::normalize_phone;
use self::helpers::{
    blank_default, local_phone_query, merge_customer_records, merge_worker_records,
    normalize_config_phone, phone_matches_normalized, supplier_access_code_for,
};

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
struct AuthIdentity {
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

    pub async fn login(&self, phone: &str, code: &str) -> Result<Principal, AuthError> {
        let normalized_phone = normalize_phone(phone).map_err(|_| AuthError::InvalidCredentials)?;
        let code = code.trim();
        let identity = self.identity.read().expect("auth identity lock").clone();

        if !identity.admin_phone.is_empty()
            && identity.admin_phone.eq_ignore_ascii_case(&normalized_phone)
            && !self.admin_code.is_empty()
            && code == self.admin_code
        {
            return Ok(Principal {
                role: PrincipalRole::Admin,
                display_name: identity.admin_name.clone(),
                legal_name: identity.admin_name,
                ref_: "admin".to_string(),
                phone: normalized_phone,
                avatar_url: String::new(),
            });
        }

        match self.infer_role(code)? {
            PrincipalRole::Supplier => self.login_supplier(&normalized_phone, code).await,
            PrincipalRole::Werka => self.login_werka(normalized_phone, code, &identity),
            PrincipalRole::Customer => self.login_customer(&normalized_phone, code).await,
            PrincipalRole::Aparatchi => self.login_aparatchi(&normalized_phone, code).await,
            PrincipalRole::Qolipchi => self.login_qolipchi(&normalized_phone, code).await,
            PrincipalRole::Admin => Err(AuthError::InvalidRole),
        }
    }

    async fn login_supplier(
        &self,
        normalized_phone: &str,
        code: &str,
    ) -> Result<Principal, AuthError> {
        let supplier_lookup = self
            .supplier_lookup
            .as_ref()
            .ok_or(AuthError::InvalidCredentials)?;
        let admin_state_lookup = self
            .admin_state_lookup
            .as_ref()
            .ok_or(AuthError::InvalidCredentials)?;

        let mut suppliers = supplier_lookup
            .search_suppliers(normalized_phone, 50)
            .await
            .map_err(|_| AuthError::Internal)?;
        if suppliers.is_empty()
            && let Some(local_phone) = local_phone_query(normalized_phone)
        {
            suppliers = supplier_lookup
                .search_suppliers(&local_phone, 50)
                .await
                .map_err(|_| AuthError::Internal)?;
        }
        if suppliers.is_empty() {
            suppliers = supplier_lookup
                .search_suppliers("", 500)
                .await
                .map_err(|_| AuthError::Internal)?;
        }

        let states = admin_state_lookup
            .list_states()
            .await
            .map_err(|_| AuthError::Internal)?;

        for supplier in suppliers {
            let state = states.get(supplier.id.trim()).cloned().unwrap_or_default();
            if state.removed || state.blocked {
                continue;
            }

            let code_value = supplier_access_code_for(&supplier, &state)?;
            if code.trim() == code_value
                && phone_matches_normalized(&supplier.phone, normalized_phone)
            {
                return Ok(Principal {
                    role: PrincipalRole::Supplier,
                    display_name: supplier.name.clone(),
                    legal_name: supplier.name,
                    ref_: supplier.id,
                    phone: supplier.phone,
                    avatar_url: String::new(),
                });
            }
        }

        Err(AuthError::InvalidCredentials)
    }

    async fn login_customer(
        &self,
        normalized_phone: &str,
        code: &str,
    ) -> Result<Principal, AuthError> {
        self.login_customer_party(normalized_phone, code, PrincipalRole::Customer)
            .await
    }

    async fn login_aparatchi(
        &self,
        normalized_phone: &str,
        code: &str,
    ) -> Result<Principal, AuthError> {
        match self
            .login_worker_by_role(normalized_phone, code, PrincipalRole::Aparatchi)
            .await
        {
            Ok(principal) => Ok(principal),
            Err(AuthError::InvalidCredentials) => {
                self.login_customer_party(normalized_phone, code, PrincipalRole::Aparatchi)
                    .await
            }
            Err(error) => Err(error),
        }
    }

    async fn login_qolipchi(
        &self,
        normalized_phone: &str,
        code: &str,
    ) -> Result<Principal, AuthError> {
        self.login_worker_by_role(normalized_phone, code, PrincipalRole::Qolipchi)
            .await
    }

    async fn login_worker_by_role(
        &self,
        normalized_phone: &str,
        code: &str,
        role: PrincipalRole,
    ) -> Result<Principal, AuthError> {
        let worker_lookup = self
            .worker_lookup
            .as_ref()
            .ok_or(AuthError::InvalidCredentials)?;
        let admin_state_lookup = self
            .admin_state_lookup
            .as_ref()
            .ok_or(AuthError::InvalidCredentials)?;

        let workers = self
            .search_workers_for_login(worker_lookup.as_ref(), normalized_phone)
            .await?;
        let states = admin_state_lookup
            .list_states()
            .await
            .map_err(|_| AuthError::Internal)?;

        for worker in workers {
            let state = states.get(worker.id.trim()).cloned().unwrap_or_default();
            if state.removed || state.blocked {
                continue;
            }
            let code_value = state.custom_code.trim();
            if code_value.is_empty() {
                continue;
            }
            if code.trim() == code_value
                && phone_matches_normalized(&worker.phone, normalized_phone)
            {
                return Ok(Principal {
                    role,
                    display_name: worker.name.clone(),
                    legal_name: worker.name,
                    ref_: worker.id,
                    phone: worker.phone,
                    avatar_url: String::new(),
                });
            }
        }

        Err(AuthError::InvalidCredentials)
    }

    async fn login_customer_party(
        &self,
        normalized_phone: &str,
        code: &str,
        role: PrincipalRole,
    ) -> Result<Principal, AuthError> {
        let customer_lookup = self
            .customer_lookup
            .as_ref()
            .ok_or(AuthError::InvalidCredentials)?;
        let admin_state_lookup = self
            .admin_state_lookup
            .as_ref()
            .ok_or(AuthError::InvalidCredentials)?;

        let customers = self
            .search_customers_for_login(customer_lookup.as_ref(), normalized_phone)
            .await?;

        let states = admin_state_lookup
            .list_states()
            .await
            .map_err(|_| AuthError::Internal)?;

        for customer in customers {
            let state = states.get(customer.id.trim()).cloned().unwrap_or_default();
            let code_value = state.custom_code.trim();
            if code_value.is_empty() {
                continue;
            }
            if code.trim() == code_value
                && phone_matches_normalized(&customer.phone, normalized_phone)
            {
                return Ok(Principal {
                    role: role.clone(),
                    display_name: customer.name.clone(),
                    legal_name: customer.name,
                    ref_: customer.id,
                    phone: customer.phone,
                    avatar_url: String::new(),
                });
            }
        }

        Err(AuthError::InvalidCredentials)
    }

    async fn search_customers_for_login(
        &self,
        customer_lookup: &dyn CustomerLookup,
        normalized_phone: &str,
    ) -> Result<Vec<CustomerRecord>, AuthError> {
        let mut customers = customer_lookup
            .search_customers(normalized_phone, 50)
            .await
            .map_err(|_| AuthError::Internal)?;
        if let Some(local_phone) = local_phone_query(normalized_phone) {
            let local_matches = customer_lookup
                .search_customers(&local_phone, 50)
                .await
                .map_err(|_| AuthError::Internal)?;
            merge_customer_records(&mut customers, local_matches);
        }
        if customers.is_empty() {
            customers = customer_lookup
                .search_customers("", 500)
                .await
                .map_err(|_| AuthError::Internal)?;
        }
        Ok(customers)
    }

    async fn search_workers_for_login(
        &self,
        worker_lookup: &dyn WorkerLookup,
        normalized_phone: &str,
    ) -> Result<Vec<WorkerRecord>, AuthError> {
        let mut workers = worker_lookup
            .search_workers(normalized_phone, 50)
            .await
            .map_err(|_| AuthError::Internal)?;
        if let Some(local_phone) = local_phone_query(normalized_phone) {
            let local_matches = worker_lookup
                .search_workers(&local_phone, 50)
                .await
                .map_err(|_| AuthError::Internal)?;
            merge_worker_records(&mut workers, local_matches);
        }
        if workers.is_empty() {
            workers = worker_lookup
                .search_workers("", 500)
                .await
                .map_err(|_| AuthError::Internal)?;
        }
        Ok(workers)
    }

    fn login_werka(
        &self,
        normalized_phone: String,
        code: &str,
        identity: &AuthIdentity,
    ) -> Result<Principal, AuthError> {
        if !identity.werka_phone.is_empty()
            && identity.werka_phone.eq_ignore_ascii_case(&normalized_phone)
            && !code.is_empty()
            && code == identity.werka_code
        {
            return Ok(Principal {
                role: PrincipalRole::Werka,
                display_name: identity.werka_name.clone(),
                legal_name: identity.werka_name.clone(),
                ref_: "werka".to_string(),
                phone: normalized_phone,
                avatar_url: String::new(),
            });
        }

        Err(AuthError::InvalidCredentials)
    }

    fn infer_role(&self, code: &str) -> Result<PrincipalRole, AuthError> {
        let trimmed = code.trim();

        if trimmed.starts_with(&self.supplier_prefix) {
            Ok(PrincipalRole::Supplier)
        } else if trimmed.starts_with(&self.werka_prefix) {
            Ok(PrincipalRole::Werka)
        } else if trimmed.starts_with("40") {
            Ok(PrincipalRole::Aparatchi)
        } else if trimmed.starts_with("50") {
            Ok(PrincipalRole::Qolipchi)
        } else if trimmed.starts_with("30") {
            Ok(PrincipalRole::Customer)
        } else {
            Err(AuthError::InvalidRole)
        }
    }
}

impl AuthConfigSink for AuthService {
    fn set_runtime_identity(
        &self,
        werka_phone: &str,
        werka_code: &str,
        werka_name: &str,
        admin_phone: &str,
        admin_name: &str,
    ) {
        let normalized_werka_phone =
            normalize_config_phone(werka_phone).unwrap_or_else(|_| werka_phone.trim().to_string());
        let normalized_admin_phone =
            normalize_config_phone(admin_phone).unwrap_or_else(|_| admin_phone.trim().to_string());
        let identity = AuthIdentity {
            werka_phone: normalized_werka_phone,
            werka_code: werka_code.trim().to_string(),
            werka_name: blank_default(werka_name, "Werka"),
            admin_phone: normalized_admin_phone,
            admin_name: blank_default(admin_name, "Admin"),
        };
        *self.identity.write().expect("auth identity lock") = identity;
    }
}
