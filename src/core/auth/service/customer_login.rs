use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::auth::ports::{CustomerLookup, CustomerRecord};

use super::helpers::{local_phone_query, merge_customer_records, phone_matches_normalized};
use super::{AuthError, AuthService};

impl AuthService {
    pub(super) async fn login_customer(
        &self,
        normalized_phone: &str,
        code: &str,
    ) -> Result<Principal, AuthError> {
        self.login_customer_party(normalized_phone, code, PrincipalRole::Customer)
            .await
    }

    pub(super) async fn login_customer_party(
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
}
