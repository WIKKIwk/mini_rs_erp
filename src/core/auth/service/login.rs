use crate::core::auth::models::{Principal, PrincipalRole};

use super::helpers::normalize_phone;
use super::{AuthError, AuthIdentity, AuthService};

impl AuthService {
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
