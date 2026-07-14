use crate::core::auth::models::{Principal, PrincipalRole};

use super::catalog;
use super::models::{Capability, CapabilityCatalogEntry, CapabilityDefinition, RoleDefinition};

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
        (PrincipalRole::Qolipchi, "qolipchi", "Qolipchi"),
        (PrincipalRole::Boyoqchi, "boyoqchi", "Bo‘yoqchi"),
        (
            PrincipalRole::MaterialTaminotchi,
            "material_taminotchi",
            "Material taminotchisi",
        ),
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
