use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::core::auth::models::PrincipalRole;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupplierRecord {
    pub id: String,
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomerRecord {
    pub id: String,
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaterialTaminotchiRecord {
    pub id: String,
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerRecord {
    pub id: String,
    pub name: String,
    pub phone: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemUserRecord {
    pub id: String,
    pub name: String,
    pub phone: String,
    pub role: PrincipalRole,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminAccessState {
    pub custom_code: String,
    pub blocked: bool,
    pub removed: bool,
}

#[async_trait]
pub trait SupplierLookup: Send + Sync {
    async fn search_suppliers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierRecord>, AuthPortError>;
}

#[async_trait]
pub trait CustomerLookup: Send + Sync {
    async fn search_customers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CustomerRecord>, AuthPortError>;
}

#[async_trait]
pub trait MaterialTaminotchiLookup: Send + Sync {
    async fn search_material_taminotchilar(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MaterialTaminotchiRecord>, AuthPortError>;
}

#[async_trait]
pub trait WorkerLookup: Send + Sync {
    async fn search_workers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WorkerRecord>, AuthPortError>;
}

#[async_trait]
pub trait SystemUserLookup: Send + Sync {
    async fn search_system_users(
        &self,
        role: PrincipalRole,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SystemUserRecord>, AuthPortError>;
}

#[async_trait]
pub trait AdminAccessStateLookup: Send + Sync {
    async fn list_states(&self) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError>;
}

pub trait AuthConfigSink: Send + Sync {
    fn set_runtime_identity(
        &self,
        werka_phone: &str,
        werka_code: &str,
        werka_name: &str,
        admin_phone: &str,
        admin_name: &str,
    );
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum AuthPortError {
    #[error("lookup failed")]
    LookupFailed,
}
