use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::RwLock;

#[path = "authz_catalog.rs"]
mod catalog;

use crate::core::auth::models::{Principal, PrincipalRole};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Capability {
    AdminAccess,
    RoleCapabilityRead,
    RoleCapabilityManage,
    AdminSettingsRead,
    AdminSettingsManage,
    WerkaAccess,
    SupplierAccess,
    CustomerAccess,
    PushTokenManage,
    SupplierAvatarManage,
    CatalogItemRead,
    CatalogItemCreate,
    CatalogItemGroupRead,
    CatalogItemGroupManage,
    CatalogItemBulkMove,
    SupplierDirectoryRead,
    SupplierDirectoryManage,
    SupplierItemAssign,
    SupplierCodeManage,
    CustomerDirectoryRead,
    CustomerDirectoryManage,
    CustomerItemAssign,
    CustomerCodeManage,
    AdminActivityRead,
    WerkaCodeManage,
    ProductionMapManage,
    ApparatusQueueRead,
    ApparatusQueueManage,
    GscaleCatalogRead,
    GscalePrint,
    RpsBatchManage,
    RezkaSplitManage,
    RawMaterialRuleManage,
    RawMaterialAssign,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct CapabilityDefinition {
    pub capability: Capability,
    pub code: &'static str,
    pub label: &'static str,
    pub default_roles: &'static [PrincipalRole],
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct CapabilityCatalogEntry {
    pub code: &'static str,
    pub label: &'static str,
    pub default_roles: Vec<PrincipalRole>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleDefinition {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_role: Option<PrincipalRole>,
    pub capability_codes: Vec<String>,
    pub system: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RoleDefinitionUpsert {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub base_role: Option<PrincipalRole>,
    pub capability_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleAssignment {
    pub principal_role: PrincipalRole,
    pub principal_ref: String,
    pub role_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assigned_apparatus: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RoleAssignmentUpsert {
    pub principal_role: PrincipalRole,
    pub principal_ref: String,
    pub role_id: String,
    #[serde(default)]
    pub assigned_apparatus: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RoleDefinitionError {
    #[error("role id is required")]
    MissingId,
    #[error("role label is required")]
    MissingLabel,
    #[error("role id is reserved")]
    ReservedId,
    #[error("role id is invalid")]
    InvalidId,
    #[error("role needs at least one capability")]
    MissingCapabilities,
    #[error("unknown capability: {0}")]
    UnknownCapability(String),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RoleAssignmentError {
    #[error("principal ref is required")]
    MissingPrincipalRef,
    #[error("role id is required")]
    MissingRoleId,
    #[error("unknown role: {0}")]
    UnknownRole(String),
    #[error("role base does not match principal role")]
    RoleBaseMismatch,
}

#[derive(Debug, thiserror::Error)]
pub enum RoleStoreError {
    #[error("role store failed")]
    StoreFailed,
}

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

pub fn capability_catalog() -> &'static [CapabilityDefinition] {
    catalog::CAPABILITY_CATALOG
}

pub fn capability_catalog_entries() -> Vec<CapabilityCatalogEntry> {
    capability_catalog()
        .iter()
        .map(|definition| CapabilityCatalogEntry {
            code: definition.code,
            label: definition.label,
            default_roles: definition.default_roles.to_vec(),
        })
        .collect()
}

pub fn capability_by_code(code: &str) -> Option<&'static CapabilityDefinition> {
    let code = code.trim();
    capability_catalog()
        .iter()
        .find(|definition| definition.code == code)
}

pub fn system_role_definitions() -> Vec<RoleDefinition> {
    let mut roles: Vec<RoleDefinition> = [
        (PrincipalRole::Admin, "admin", "Admin"),
        (PrincipalRole::Werka, "werka", "Werka"),
        (PrincipalRole::Supplier, "supplier", "Supplier"),
        (PrincipalRole::Customer, "customer", "Customer"),
    ]
    .into_iter()
    .map(|(role, id, label)| RoleDefinition {
        id: id.to_string(),
        label: label.to_string(),
        capability_codes: capability_codes_for_role(role.clone()),
        base_role: Some(role),
        system: true,
    })
    .collect();
    roles.push(RoleDefinition {
        id: "aparatchi".to_string(),
        label: "Aparatchi".to_string(),
        capability_codes: vec![
            capability_code(Capability::ApparatusQueueRead)
                .unwrap_or("apparatus.queue.read")
                .to_string(),
            capability_code(Capability::ApparatusQueueManage)
                .unwrap_or("apparatus.queue.manage")
                .to_string(),
        ],
        base_role: None,
        system: true,
    });
    roles
}

pub fn normalize_custom_role(
    input: RoleDefinitionUpsert,
) -> Result<RoleDefinition, RoleDefinitionError> {
    let id = input.id.trim().to_ascii_lowercase();
    if id.is_empty() {
        return Err(RoleDefinitionError::MissingId);
    }
    if system_role_ids().contains(id.as_str()) {
        return Err(RoleDefinitionError::ReservedId);
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(RoleDefinitionError::InvalidId);
    }

    let label = input.label.trim().to_string();
    if label.is_empty() {
        return Err(RoleDefinitionError::MissingLabel);
    }

    let requested: BTreeSet<String> = input
        .capability_codes
        .into_iter()
        .map(|code| code.trim().to_string())
        .filter(|code| !code.is_empty())
        .collect();
    if requested.is_empty() {
        return Err(RoleDefinitionError::MissingCapabilities);
    }
    for code in &requested {
        if capability_by_code(code).is_none() {
            return Err(RoleDefinitionError::UnknownCapability(code.clone()));
        }
    }

    let capability_codes = capability_catalog()
        .iter()
        .filter(|definition| requested.contains(definition.code))
        .map(|definition| definition.code.to_string())
        .collect();

    Ok(RoleDefinition {
        id,
        label,
        base_role: None,
        capability_codes,
        system: false,
    })
}

pub fn normalize_role_assignment(
    input: RoleAssignmentUpsert,
    roles: &[RoleDefinition],
) -> Result<RoleAssignment, RoleAssignmentError> {
    let principal_ref = input.principal_ref.trim().to_string();
    if principal_ref.is_empty() {
        return Err(RoleAssignmentError::MissingPrincipalRef);
    }
    let role_id = input.role_id.trim().to_ascii_lowercase();
    if role_id.is_empty() {
        return Err(RoleAssignmentError::MissingRoleId);
    }
    let Some(role) = roles.iter().find(|role| role.id == role_id) else {
        return Err(RoleAssignmentError::UnknownRole(role_id));
    };
    if let Some(base_role) = &role.base_role
        && role.system
        && base_role != &input.principal_role
    {
        return Err(RoleAssignmentError::RoleBaseMismatch);
    }
    Ok(RoleAssignment {
        principal_role: input.principal_role,
        principal_ref,
        role_id,
        assigned_apparatus: normalize_assigned_apparatus(input.assigned_apparatus),
    })
}

pub fn role_assignment_key(role: &PrincipalRole, ref_: &str) -> String {
    format!("{}:{}", role_key(role), ref_.trim())
}

pub fn capability_codes_for_role(role: PrincipalRole) -> Vec<String> {
    capability_catalog()
        .iter()
        .filter(|definition| definition.default_roles.contains(&role))
        .map(|definition| definition.code.to_string())
        .collect()
}

pub fn capability_code(capability: Capability) -> Option<&'static str> {
    capability_catalog()
        .iter()
        .find(|definition| definition.capability == capability)
        .map(|definition| definition.code)
}

pub fn has_capability(principal: &Principal, capability: Capability) -> bool {
    capability_catalog()
        .iter()
        .find(|definition| definition.capability == capability)
        .map(|definition| definition.default_roles.contains(&principal.role))
        .unwrap_or(false)
}

fn role_key(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Admin => "admin",
    }
}

fn system_role_ids() -> BTreeSet<&'static str> {
    ["admin", "werka", "supplier", "customer", "aparatchi"]
        .into_iter()
        .collect()
}

fn normalize_assigned_apparatus(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
#[path = "authz_tests.rs"]
mod tests;
