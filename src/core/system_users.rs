use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::core::auth::models::PrincipalRole;
use crate::core::auth::ports::{AuthPortError, SystemUserLookup, SystemUserRecord};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemUser {
    pub id: String,
    pub role: PrincipalRole,
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemUserUpsert {
    #[serde(default)]
    pub id: String,
    pub role: PrincipalRole,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub phone: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SystemUserError {
    #[error("system user name is required")]
    MissingName,
    #[error("system user phone is required")]
    MissingPhone,
    #[error("system user role is not supported")]
    InvalidRole,
    #[error("system user phone already exists")]
    DuplicatePhone,
    #[error("system user not found")]
    NotFound,
    #[error("system user store failed")]
    StoreFailed,
}

#[async_trait]
pub trait SystemUserStorePort: Send + Sync {
    async fn users(
        &self,
        role: &PrincipalRole,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SystemUser>, SystemUserError>;
    async fn users_by_ids(&self, ids: &[String]) -> Result<Vec<SystemUser>, SystemUserError>;
    async fn upsert_user(&self, user: SystemUser) -> Result<SystemUser, SystemUserError>;
}

#[derive(Clone)]
pub struct SystemUserService {
    store: Arc<dyn SystemUserStorePort>,
}

impl SystemUserService {
    pub fn new(store: Arc<dyn SystemUserStorePort>) -> Self {
        Self { store }
    }

    pub fn unavailable() -> Self {
        Self::new(Arc::new(UnavailableSystemUserStore))
    }

    pub async fn users(
        &self,
        role: &PrincipalRole,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SystemUser>, SystemUserError> {
        validate_role(role)?;
        self.store.users(role, query, limit.clamp(1, 500)).await
    }

    pub async fn users_by_ids(&self, ids: &[String]) -> Result<Vec<SystemUser>, SystemUserError> {
        self.store.users_by_ids(ids).await
    }

    pub async fn upsert_user(
        &self,
        input: SystemUserUpsert,
    ) -> Result<SystemUser, SystemUserError> {
        validate_role(&input.role)?;
        let name = input.name.trim();
        if name.is_empty() {
            return Err(SystemUserError::MissingName);
        }
        let phone = input.phone.trim();
        if phone.is_empty() {
            return Err(SystemUserError::MissingPhone);
        }
        let id = if input.id.trim().is_empty() {
            new_system_user_id(&input.role)
        } else {
            input.id.trim().to_string()
        };
        self.store
            .upsert_user(SystemUser {
                id,
                role: input.role,
                name: name.to_string(),
                phone: phone.to_string(),
            })
            .await
    }
}

#[async_trait]
impl SystemUserLookup for SystemUserService {
    async fn search_system_users(
        &self,
        role: PrincipalRole,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SystemUserRecord>, AuthPortError> {
        self.users(&role, query, limit)
            .await
            .map(|users| {
                users
                    .into_iter()
                    .map(|user| SystemUserRecord {
                        id: user.id,
                        name: user.name,
                        phone: user.phone,
                        role: user.role,
                    })
                    .collect()
            })
            .map_err(|_| AuthPortError::LookupFailed)
    }
}

fn validate_role(role: &PrincipalRole) -> Result<(), SystemUserError> {
    match role {
        PrincipalRole::Qolipchi | PrincipalRole::Boyoqchi => Ok(()),
        _ => Err(SystemUserError::InvalidRole),
    }
}

fn new_system_user_id(role: &PrincipalRole) -> String {
    let bytes: [u8; 12] = rand::random();
    let prefix = match role {
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Boyoqchi => "boyoqchi",
        _ => "system_user",
    };
    format!("{prefix}_{}", data_encoding::HEXLOWER.encode(&bytes))
}

struct UnavailableSystemUserStore;

#[async_trait]
impl SystemUserStorePort for UnavailableSystemUserStore {
    async fn users(
        &self,
        _role: &PrincipalRole,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<SystemUser>, SystemUserError> {
        Err(SystemUserError::StoreFailed)
    }

    async fn users_by_ids(&self, _ids: &[String]) -> Result<Vec<SystemUser>, SystemUserError> {
        Err(SystemUserError::StoreFailed)
    }

    async fn upsert_user(&self, _user: SystemUser) -> Result<SystemUser, SystemUserError> {
        Err(SystemUserError::StoreFailed)
    }
}

#[derive(Default)]
pub struct MemorySystemUserStore {
    users: RwLock<Vec<SystemUser>>,
}

impl MemorySystemUserStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SystemUserStorePort for MemorySystemUserStore {
    async fn users(
        &self,
        role: &PrincipalRole,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SystemUser>, SystemUserError> {
        let needle = query.trim().to_lowercase();
        let mut users = self.users.read().await.clone();
        users.sort_by_key(|user| user.name.to_lowercase());
        Ok(users
            .into_iter()
            .filter(|user| {
                user.role == *role
                    && (needle.is_empty()
                        || user.name.to_lowercase().contains(&needle)
                        || user.phone.to_lowercase().contains(&needle))
            })
            .take(limit)
            .collect())
    }

    async fn users_by_ids(&self, ids: &[String]) -> Result<Vec<SystemUser>, SystemUserError> {
        let users = self.users.read().await;
        Ok(ids
            .iter()
            .filter_map(|id| users.iter().find(|user| user.id == id.trim()).cloned())
            .collect())
    }

    async fn upsert_user(&self, user: SystemUser) -> Result<SystemUser, SystemUserError> {
        let mut users = self.users.write().await;
        if users.iter().any(|existing| {
            existing.id != user.id
                && existing.role == user.role
                && normalized_phone(&existing.phone) == normalized_phone(&user.phone)
        }) {
            return Err(SystemUserError::DuplicatePhone);
        }
        if let Some(existing) = users.iter_mut().find(|existing| existing.id == user.id) {
            *existing = user.clone();
        } else {
            users.push(user.clone());
        }
        Ok(user)
    }
}

fn normalized_phone(value: &str) -> String {
    value.chars().filter(char::is_ascii_digit).collect()
}
