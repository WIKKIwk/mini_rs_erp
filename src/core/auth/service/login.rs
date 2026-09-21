use crate::core::auth::models::{Principal, PrincipalRole};

use super::helpers::{is_numeric_access_code, normalize_phone, requires_numeric_access_code};
use super::{AuthError, AuthIdentity, AuthService};

impl AuthService {
    pub async fn login(&self, phone: &str, code: &str) -> Result<Principal, AuthError> {
        let normalized_phone = normalize_phone(phone).map_err(|_| AuthError::InvalidCredentials)?;
        let attempts = self
            .login_throttle
            .lock()
            .map_err(|_| AuthError::Internal)?
            .for_phone(&normalized_phone, std::time::Instant::now())?;
        // Serialize authentication for this phone so parallel failures cannot
        // bypass the limit, without counting successful requests as attempts.
        let mut attempts = attempts.lock().await;
        attempts.check(std::time::Instant::now())?;
        let result = self.authenticate(normalized_phone, code.trim()).await;
        match &result {
            Ok(principal) => {
                attempts.reset();
                if let Some(store) = &self.admin_state_lookup {
                    // A recovery failure must not turn a correct login into a failure.
                    if store.remember_access_code(&principal.ref_, code.trim()).await.is_err() {
                        tracing::warn!("could not preserve verified access code for admin display");
                    }
                }
            }
            Err(AuthError::InvalidCredentials | AuthError::InvalidRole) => {
                attempts.record_failure(std::time::Instant::now());
            }
            Err(_) => {}
        }
        result
    }

    async fn authenticate(
        &self,
        normalized_phone: String,
        code: &str,
    ) -> Result<Principal, AuthError> {
        let identity = self.identity.read().expect("auth identity lock").clone();

        if let Some(principal) = self
            .login_builtin(
                "admin",
                PrincipalRole::Admin,
                &normalized_phone,
                code,
                (
                    &identity.admin_phone,
                    &identity.admin_name,
                    &self.admin_code,
                ),
            )
            .await?
        {
            return Ok(principal);
        }

        let role = self.infer_role(code)?;
        if requires_numeric_access_code(&role) && !is_numeric_access_code(code) {
            return Err(AuthError::InvalidCredentials);
        }

        match role {
            PrincipalRole::Supplier => self.login_supplier(&normalized_phone, code).await,
            PrincipalRole::Werka => self.login_werka(normalized_phone, code, &identity).await,
            PrincipalRole::Customer => self.login_customer(&normalized_phone, code).await,
            PrincipalRole::Aparatchi => self.login_aparatchi(&normalized_phone, code).await,
            PrincipalRole::Qolipchi => self.login_qolipchi(&normalized_phone, code).await,
            PrincipalRole::Boyoqchi => self.login_boyoqchi(&normalized_phone, code).await,
            PrincipalRole::TayyorlovMasteri => {
                self.login_system_user_by_role(&normalized_phone, code, role)
                    .await
            }
            PrincipalRole::HomashyoRezkachi => {
                self.login_system_user_by_role(&normalized_phone, code, role)
                    .await
            }
            PrincipalRole::MaterialTaminotchi => {
                self.login_material_taminotchi(normalized_phone, code, &identity)
                    .await
            }
            PrincipalRole::Admin => Err(AuthError::InvalidRole),
        }
    }

    async fn login_werka(
        &self,
        normalized_phone: String,
        code: &str,
        identity: &AuthIdentity,
    ) -> Result<Principal, AuthError> {
        self.login_builtin(
            "werka",
            PrincipalRole::Werka,
            &normalized_phone,
            code,
            (
                &identity.werka_phone,
                &identity.werka_name,
                &identity.werka_code,
            ),
        )
        .await?
        .ok_or(AuthError::InvalidCredentials)
    }

    async fn login_material_taminotchi(
        &self,
        normalized_phone: String,
        code: &str,
        identity: &AuthIdentity,
    ) -> Result<Principal, AuthError> {
        if let Some(principal) = self
            .login_builtin(
                "material_taminotchi",
                PrincipalRole::MaterialTaminotchi,
                &normalized_phone,
                code,
                (
                    &identity.material_taminotchi_phone,
                    &identity.material_taminotchi_name,
                    &identity.material_taminotchi_code,
                ),
            )
            .await?
        {
            return Ok(principal);
        }

        match self
            .login_material_taminotchi_party(&normalized_phone, code)
            .await
        {
            Ok(principal) => Ok(principal),
            Err(AuthError::InvalidCredentials) if code.trim().starts_with("60") => {
                self.login_customer_party(
                    &normalized_phone,
                    code,
                    PrincipalRole::MaterialTaminotchi,
                )
                .await
            }
            Err(error) => Err(error),
        }
    }

    async fn login_builtin(
        &self,
        ref_: &str,
        role: PrincipalRole,
        phone: &str,
        code: &str,
        fallback: (&str, &str, &str),
    ) -> Result<Option<Principal>, AuthError> {
        let mut identity = crate::core::auth::ports::BuiltinLoginIdentity {
            phone: fallback.0.to_string(),
            name: fallback.1.to_string(),
        };
        let mut hash = fallback.2.to_string();
        if let Some(lookup) = &self.admin_state_lookup {
            if let Some(stored) = lookup
                .builtin_identity(ref_)
                .await
                .map_err(|_| AuthError::Internal)?
            {
                identity = stored;
                let states = lookup
                    .list_states()
                    .await
                    .map_err(|_| AuthError::Internal)?;
                let state = states.get(ref_).ok_or(AuthError::Internal)?;
                if state.blocked || state.removed {
                    return Ok(None);
                }
                hash = state.custom_code.clone();
            } else if self.builtin_credentials_from_store {
                return Ok(None);
            }
        }
        if !super::helpers::phone_matches_normalized(&identity.phone, phone)
            || !super::helpers::code_matches(&hash, code).await?
        {
            return Ok(None);
        }
        Ok(Some(Principal {
            role,
            display_name: identity.name.clone(),
            legal_name: identity.name,
            ref_: ref_.to_string(),
            phone: phone.to_string(),
            avatar_url: String::new(),
        }))
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
        } else if trimmed.starts_with("80") {
            Ok(PrincipalRole::Boyoqchi)
        } else if trimmed.starts_with("91") {
            Ok(PrincipalRole::HomashyoRezkachi)
        } else if trimmed.starts_with("90") {
            Ok(PrincipalRole::TayyorlovMasteri)
        } else if trimmed.starts_with("70") || trimmed.starts_with("60") {
            Ok(PrincipalRole::MaterialTaminotchi)
        } else if trimmed.starts_with("30") {
            Ok(PrincipalRole::Customer)
        } else {
            Err(AuthError::InvalidRole)
        }
    }
}
