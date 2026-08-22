mod models;
mod normalize;
mod queries;
mod store;

#[path = "authz_catalog.rs"]
mod catalog;

pub use models::{
    Capability, CapabilityCatalogEntry, CapabilityDefinition, RoleAssignment, RoleAssignmentError,
    RoleAssignmentUpsert, RoleDefinition, RoleDefinitionError, RoleDefinitionUpsert,
    RoleStoreError,
};
pub use normalize::{normalize_custom_role, normalize_role_assignment, role_assignment_key};
pub use queries::{
    capability_by_code, capability_catalog, capability_catalog_entries, capability_code,
    capability_codes_for_role, has_capability, system_role_definitions,
};
pub use store::{MemoryRoleDefinitionStore, RoleDefinitionStorePort};

#[cfg(test)]
#[path = "authz_tests.rs"]
mod tests;
