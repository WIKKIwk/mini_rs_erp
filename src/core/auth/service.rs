use std::sync::Arc;
use std::sync::RwLock;

use crate::config::AppConfig;
use crate::core::auth::access_codes::{SupplierAccessInput, supplier_access_code};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::auth::ports::{
    AdminAccessState, AdminAccessStateLookup, AuthConfigSink, CustomerLookup, CustomerRecord,
    SupplierLookup, SupplierRecord,
};

#[derive(Clone)]
pub struct AuthService {
    supplier_prefix: String,
    werka_prefix: String,
    identity: Arc<RwLock<AuthIdentity>>,
    admin_code: String,
    supplier_lookup: Option<Arc<dyn SupplierLookup>>,
    customer_lookup: Option<Arc<dyn CustomerLookup>>,
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
            admin_state_lookup: None,
        }
    }

    #[cfg(test)]
    pub fn with_supplier_dependencies(
        mut self,
        supplier_lookup: Arc<dyn SupplierLookup>,
        admin_state_lookup: Arc<dyn AdminAccessStateLookup>,
    ) -> Self {
        self.supplier_lookup = Some(supplier_lookup);
        self.admin_state_lookup = Some(admin_state_lookup);
        self
    }

    #[cfg(test)]
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
        self.login_customer_party(normalized_phone, code, PrincipalRole::Aparatchi)
            .await
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

pub fn normalize_phone(input: &str) -> Result<String, AuthError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AuthError::InvalidCredentials);
    }

    let mut digits = String::new();
    for ch in trimmed.chars() {
        if ch == '+' {
            continue;
        }
        if !ch.is_ascii_digit() {
            return Err(AuthError::InvalidCredentials);
        }
        digits.push(ch);
    }

    if !trimmed.starts_with('+') && digits.len() == 9 {
        digits = format!("998{digits}");
    }

    if digits.len() < 9 || digits.len() > 12 {
        return Err(AuthError::InvalidCredentials);
    }

    Ok(format!("+{digits}"))
}

fn normalize_config_phone(phone: &str) -> Result<String, AuthError> {
    let mut clean = phone.replace([' ', '-', '(', ')'], "");

    if !clean.trim().starts_with('+') && clean.len() == 9 {
        clean = format!("998{clean}");
    }

    normalize_phone(&clean)
}

fn blank_default(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn phone_matches_normalized(stored_phone: &str, normalized_login_phone: &str) -> bool {
    normalize_phone(stored_phone)
        .map(|phone| phone.eq_ignore_ascii_case(normalized_login_phone))
        .unwrap_or_else(|_| {
            stored_phone
                .trim()
                .eq_ignore_ascii_case(normalized_login_phone)
        })
}

fn merge_customer_records(customers: &mut Vec<CustomerRecord>, extra: Vec<CustomerRecord>) {
    for record in extra {
        if customers
            .iter()
            .any(|existing| existing.id.trim() == record.id.trim())
        {
            continue;
        }
        customers.push(record);
    }
}

fn local_phone_query(normalized_phone: &str) -> Option<String> {
    let digits = normalized_phone.trim().strip_prefix('+')?;
    (digits.len() == 12 && digits.starts_with("998")).then(|| digits[3..].to_string())
}

fn supplier_access_code_for(
    supplier: &SupplierRecord,
    state: &AdminAccessState,
) -> Result<String, AuthError> {
    let custom = state.custom_code.trim();
    if !custom.is_empty() {
        return Ok(custom.to_string());
    }

    supplier_access_code(&SupplierAccessInput {
        ref_: supplier.id.clone(),
        name: supplier.name.clone(),
        phone: supplier.phone.clone(),
    })
}
