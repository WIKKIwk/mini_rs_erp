use std::collections::BTreeMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::core::auth::models::PrincipalRole;

use super::models::{RoleAssignment, RoleDefinition, RoleStoreError};
use super::normalize::role_assignment_key;

#[async_trait]
pub trait RoleDefinitionStorePort: Send + Sync {
    async fn role_definitions(&self) -> Result<Vec<RoleDefinition>, RoleStoreError>;
    async fn put_role_definition(&self, role: RoleDefinition) -> Result<(), RoleStoreError>;
    async fn role_assignments(&self) -> Result<Vec<RoleAssignment>, RoleStoreError>;
    async fn put_role_assignment(&self, assignment: RoleAssignment) -> Result<(), RoleStoreError>;
    async fn delete_role_assignment(
        &self,
        role: &PrincipalRole,
        ref_: &str,
    ) -> Result<(), RoleStoreError>;
}

pub struct MemoryRoleDefinitionStore {
    roles: RwLock<BTreeMap<String, RoleDefinition>>,
    assignments: RwLock<BTreeMap<String, RoleAssignment>>,
}

impl MemoryRoleDefinitionStore {
    pub fn new() -> Self {
        Self {
            roles: RwLock::new(BTreeMap::new()),
            assignments: RwLock::new(BTreeMap::new()),
        }
    }
}

impl Default for MemoryRoleDefinitionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RoleDefinitionStorePort for MemoryRoleDefinitionStore {
    async fn role_definitions(&self) -> Result<Vec<RoleDefinition>, RoleStoreError> {
        Ok(self.roles.read().await.values().cloned().collect())
    }

    async fn put_role_definition(&self, role: RoleDefinition) -> Result<(), RoleStoreError> {
        self.roles.write().await.insert(role.id.clone(), role);
        Ok(())
    }

    async fn role_assignments(&self) -> Result<Vec<RoleAssignment>, RoleStoreError> {
        Ok(self.assignments.read().await.values().cloned().collect())
    }

    async fn put_role_assignment(&self, assignment: RoleAssignment) -> Result<(), RoleStoreError> {
        self.assignments.write().await.insert(
            role_assignment_key(&assignment.principal_role, &assignment.principal_ref),
            assignment,
        );
        Ok(())
    }

    async fn delete_role_assignment(
        &self,
        role: &PrincipalRole,
        ref_: &str,
    ) -> Result<(), RoleStoreError> {
        self.assignments
            .write()
            .await
            .remove(&role_assignment_key(role, ref_));
        Ok(())
    }
}
