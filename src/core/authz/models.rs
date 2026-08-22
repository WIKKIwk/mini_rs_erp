use serde::{Deserialize, Serialize};

use crate::core::apparatus_standard::ApparatusId;
use crate::core::auth::models::PrincipalRole;

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
    BoyoqchiAccess,
    ReturnedPaintRequestCreate,
    ReturnedPaintRequestRead,
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
    FactoryLocationManage,
    InventoryMovementManage,
    ApparatusQueueRead,
    ApparatusQueueManage,
    GscaleCatalogRead,
    GscalePrint,
    RpsBatchManage,
    RezkaSplitManage,
    QolipManage,
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
    /// Canonical apparatus IDs serialized as strings at the shared API boundary.
    /// Values are validated by `normalize_role_assignment`; display titles are
    /// never accepted as authorization scopes.
    pub assigned_apparatus: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assigned_item_groups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RoleAssignmentUpsert {
    pub principal_role: PrincipalRole,
    pub principal_ref: String,
    pub role_id: String,
    #[serde(default)]
    pub assigned_apparatus: Vec<String>,
    #[serde(default)]
    pub assigned_item_groups: Vec<String>,
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
    #[error("material taminotchi needs at least one item group")]
    MissingAssignedItemGroups,
    #[error("assigned apparatus is not a canonical apparatus id: {0}")]
    InvalidAssignedApparatus(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RoleStoreError {
    #[error("role store failed")]
    StoreFailed,
}

impl RoleAssignment {
    pub fn allows_apparatus(&self, apparatus_id: &ApparatusId) -> bool {
        self.assigned_apparatus
            .iter()
            .any(|assigned| assigned == apparatus_id.as_str())
    }

    pub fn apparatus_scope(&self) -> Result<Vec<ApparatusId>, RoleAssignmentError> {
        self.assigned_apparatus
            .iter()
            .map(|value| {
                ApparatusId::new(value.clone())
                    .map_err(|_| RoleAssignmentError::InvalidAssignedApparatus(value.clone()))
            })
            .collect()
    }
}
