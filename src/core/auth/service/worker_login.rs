use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::auth::ports::{SystemUserLookup, SystemUserRecord, WorkerLookup, WorkerRecord};

use super::helpers::{local_phone_query, merge_worker_records, phone_matches_normalized};
use super::{AuthError, AuthService};

impl AuthService {
    pub(super) async fn login_aparatchi(
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

    pub(super) async fn login_qolipchi(
        &self,
        normalized_phone: &str,
        code: &str,
    ) -> Result<Principal, AuthError> {
        self.login_system_user_by_role(
            normalized_phone,
            code,
            PrincipalRole::Qolipchi,
        )
        .await
    }

    pub(super) async fn login_boyoqchi(
        &self,
        normalized_phone: &str,
        code: &str,
    ) -> Result<Principal, AuthError> {
        self.login_system_user_by_role(
            normalized_phone,
            code,
            PrincipalRole::Boyoqchi,
        )
        .await
    }

    async fn login_system_user_by_role(
        &self,
        normalized_phone: &str,
        code: &str,
        role: PrincipalRole,
    ) -> Result<Principal, AuthError> {
        let lookup = self
            .system_user_lookup
            .as_ref()
            .ok_or(AuthError::InvalidCredentials)?;
        let states = self
            .admin_state_lookup
            .as_ref()
            .ok_or(AuthError::InvalidCredentials)?
            .list_states()
            .await
            .map_err(|_| AuthError::Internal)?;
        let users = self
            .search_system_users_for_login(lookup.as_ref(), normalized_phone, role.clone())
            .await?;

        for user in users {
            let state = states.get(user.id.trim()).cloned().unwrap_or_default();
            if state.removed || state.blocked || user.role != role {
                continue;
            }
            if !state.custom_code.trim().is_empty()
                && code.trim() == state.custom_code.trim()
                && phone_matches_normalized(&user.phone, normalized_phone)
            {
                return Ok(Principal {
                    role,
                    display_name: user.name.clone(),
                    legal_name: user.name,
                    ref_: user.id,
                    phone: user.phone,
                    avatar_url: String::new(),
                });
            }
        }

        Err(AuthError::InvalidCredentials)
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

    async fn search_system_users_for_login(
        &self,
        lookup: &dyn SystemUserLookup,
        normalized_phone: &str,
        role: PrincipalRole,
    ) -> Result<Vec<SystemUserRecord>, AuthError> {
        let mut users = lookup
            .search_system_users(role.clone(), normalized_phone, 50)
            .await
            .map_err(|_| AuthError::Internal)?;
        if let Some(local_phone) = local_phone_query(normalized_phone) {
            let local_matches = lookup
                .search_system_users(role.clone(), &local_phone, 50)
                .await
                .map_err(|_| AuthError::Internal)?;
            for user in local_matches {
                if !users.iter().any(|existing| existing.id == user.id) {
                    users.push(user);
                }
            }
        }
        if users.is_empty() {
            users = lookup
                .search_system_users(role, "", 500)
                .await
                .map_err(|_| AuthError::Internal)?;
        }
        Ok(users)
    }
}
