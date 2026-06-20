use crate::core::auth::models::{Principal, PrincipalRole};

use super::helpers::{local_phone_query, phone_matches_normalized, supplier_access_code_for};
use super::{AuthError, AuthService};

impl AuthService {
    pub(super) async fn login_supplier(
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
}
