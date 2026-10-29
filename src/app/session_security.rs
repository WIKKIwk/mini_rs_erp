use std::sync::Arc;

use async_trait::async_trait;

use crate::core::admin::ports::{AdminPortError, AdminSessionRevoker};
use crate::core::admin::service::AdminService;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::access::SessionAccessValidator;
use crate::core::session::manager::SessionManager;
use crate::error::AppError;

pub(super) fn protect_sessions(
    admin: &mut AdminService,
    sessions: SessionManager,
) -> SessionManager {
    let sessions = sessions.with_access_validator(Arc::new(admin.clone()));
    *admin = admin
        .clone()
        .with_session_revoker(Arc::new(sessions.clone()));
    sessions
}

#[async_trait]
impl SessionAccessValidator for AdminService {
    async fn is_active(&self, principal: &Principal) -> Result<bool, AppError> {
        self.principal_is_active(principal)
            .await
            .map_err(|_| AppError::Storage("account access state unavailable".into()))
    }
}

#[async_trait]
impl AdminSessionRevoker for SessionManager {
    async fn revoke_principal_sessions(&self, principal_ref: &str) -> Result<(), AdminPortError> {
        // A directory identity can have sessions under more than one role.
        for role in [
            PrincipalRole::Supplier,
            PrincipalRole::Werka,
            PrincipalRole::Customer,
            PrincipalRole::Aparatchi,
            PrincipalRole::Qolipchi,
            PrincipalRole::Boyoqchi,
            PrincipalRole::MaterialTaminotchi,
            PrincipalRole::Admin,
            PrincipalRole::TayyorlovMasteri,
            PrincipalRole::HomashyoRezkachi,
            PrincipalRole::SalesManager,
            PrincipalRole::Brigader,
        ] {
            self.delete_for_principal(&role, principal_ref)
                .await
                .map_err(|_| AdminPortError::LookupFailed)?;
        }
        Ok(())
    }
}
