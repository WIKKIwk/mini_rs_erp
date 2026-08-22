use std::collections::BTreeSet;

use crate::core::auth::models::PrincipalRole;
use crate::core::apparatus_groups::apparatus_id_for_name;

use super::models::{
    RoleAssignment, RoleAssignmentError, RoleAssignmentUpsert, RoleDefinition, RoleDefinitionError,
    RoleDefinitionUpsert,
};
use super::queries::{capability_by_code, capability_catalog};

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
    let assigned_item_groups = normalize_assigned_item_groups(input.assigned_item_groups);
    if input.principal_role == PrincipalRole::MaterialTaminotchi
        && role.id == "material_taminotchi"
        && assigned_item_groups.is_empty()
    {
        return Err(RoleAssignmentError::MissingAssignedItemGroups);
    }
    Ok(RoleAssignment {
        principal_role: input.principal_role,
        principal_ref,
        role_id,
        assigned_apparatus: normalize_assigned_apparatus(input.assigned_apparatus),
        assigned_item_groups,
    })
}

pub fn role_assignment_key(role: &PrincipalRole, ref_: &str) -> String {
    format!("{}:{}", role_key(role), ref_.trim())
}

/// Returns the canonical apparatus identity used by authorization checks.
///
/// Legacy role assignments may still contain display names, so names are
/// resolved through the same deterministic identity function used by the
/// apparatus catalog. A value that is already an apparatus id is accepted as
/// an id and normalized without treating display-name aliases as equivalent.
pub fn canonical_apparatus_id(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    if value.starts_with("apparatus:") {
        return Some(value.to_ascii_lowercase());
    }
    Some(apparatus_id_for_name(value))
}

pub fn assigned_apparatus_contains(requested: &str, assigned: &[String]) -> bool {
    let Some(requested_id) = canonical_apparatus_id(requested) else {
        return false;
    };
    assigned
        .iter()
        .filter_map(|value| canonical_apparatus_id(value))
        .any(|id| id == requested_id)
}

fn role_key(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Boyoqchi => "boyoqchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}

fn system_role_ids() -> BTreeSet<&'static str> {
    [
        "admin",
        "werka",
        "supplier",
        "customer",
        "aparatchi",
        "qolipchi",
        "boyoqchi",
        "material_taminotchi",
    ]
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

fn normalize_assigned_item_groups(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
