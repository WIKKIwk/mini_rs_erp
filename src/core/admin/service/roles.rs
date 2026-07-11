use std::collections::{BTreeSet, VecDeque};

use super::*;
use crate::core::admin::service::helpers::dedupe_strings;

impl AdminService {
    pub fn with_role_store(mut self, role_store: Arc<dyn RoleDefinitionStorePort>) -> Self {
        self.role_store = role_store;
        self
    }

    pub async fn role_definitions(&self) -> Result<Vec<RoleDefinition>, AdminPortError> {
        self.all_role_definitions().await
    }

    async fn all_role_definitions(&self) -> Result<Vec<RoleDefinition>, AdminPortError> {
        let mut roles = system_role_definitions();
        let system_role_ids: std::collections::BTreeSet<String> =
            roles.iter().map(|role| role.id.clone()).collect();
        roles.extend(
            self.role_store
                .role_definitions()
                .await
                .map_err(|_| AdminPortError::LookupFailed)?
                .into_iter()
                .filter(|role| !system_role_ids.contains(&role.id)),
        );
        roles.sort_by(|left, right| {
            left.system
                .cmp(&right.system)
                .reverse()
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(roles)
    }

    pub async fn role_assignments(&self) -> Result<Vec<RoleAssignment>, AdminPortError> {
        self.role_store
            .role_assignments()
            .await
            .map_err(|_| AdminPortError::LookupFailed)
    }

    pub async fn upsert_role_definition(
        &self,
        input: RoleDefinitionUpsert,
    ) -> Result<RoleDefinition, AdminPortError> {
        let role = normalize_custom_role(input)
            .map_err(|error| AdminPortError::InvalidInput(error.to_string()))?;
        self.role_store
            .put_role_definition(role.clone())
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        Ok(role)
    }

    pub async fn upsert_role_assignment(
        &self,
        input: RoleAssignmentUpsert,
    ) -> Result<RoleAssignment, AdminPortError> {
        let roles = self.all_role_definitions().await?;
        let assignment = normalize_role_assignment(input, &roles)
            .map_err(|error| AdminPortError::InvalidInput(error.to_string()))?;
        self.ensure_system_role_assignment_uses_native_principal(&assignment)
            .await?;
        self.role_store
            .put_role_assignment(assignment.clone())
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        Ok(assignment)
    }

    pub async fn delete_role_assignment(
        &self,
        role: &PrincipalRole,
        ref_: &str,
    ) -> Result<(), AdminPortError> {
        self.role_store
            .delete_role_assignment(role, ref_)
            .await
            .map_err(|_| AdminPortError::LookupFailed)
    }

    pub async fn principal_has_capability(
        &self,
        principal: &Principal,
        capability: Capability,
    ) -> bool {
        match self.principal_assigned_role(principal).await {
            Ok(Some(role)) => capability_code(capability)
                .map(|code| role.capability_codes.iter().any(|item| item == code))
                .unwrap_or(false),
            Ok(None) => has_capability(principal, capability),
            Err(_) => false,
        }
    }

    pub async fn principal_capability_codes(&self, principal: &Principal) -> Vec<String> {
        match self.principal_assigned_role(principal).await {
            Ok(Some(role)) => role.capability_codes,
            Ok(None) => capability_codes_for_role(principal.role.clone()),
            Err(_) => Vec::new(),
        }
    }

    pub async fn principal_assigned_apparatus(&self, principal: &Principal) -> Vec<String> {
        match self.principal_assignment(principal).await {
            Ok(Some(assignment)) => assignment.assigned_apparatus,
            _ => Vec::new(),
        }
    }

    pub async fn principal_assigned_item_groups(&self, principal: &Principal) -> Vec<String> {
        match self.principal_assignment(principal).await {
            Ok(Some(assignment)) => assignment.assigned_item_groups,
            _ => Vec::new(),
        }
    }

    pub async fn principal_assigned_item_group_scope(
        &self,
        principal: &Principal,
    ) -> Result<Vec<String>, AdminPortError> {
        let Some(assignment) = self.principal_assignment(principal).await? else {
            return Ok(Vec::new());
        };
        self.item_group_scope(assignment.assigned_item_groups).await
    }

    pub async fn item_group_scope(
        &self,
        groups: Vec<String>,
    ) -> Result<Vec<String>, AdminPortError> {
        let groups = dedupe_strings(groups);
        if groups.is_empty() {
            return Ok(Vec::new());
        }
        let tree = self.item_group_tree().await?;
        Ok(expand_item_groups_with_descendants(groups, &tree))
    }

    async fn principal_assigned_role(
        &self,
        principal: &Principal,
    ) -> Result<Option<RoleDefinition>, AdminPortError> {
        let Some(assignment) = self.principal_assignment(principal).await? else {
            return Ok(None);
        };
        self.all_role_definitions()
            .await?
            .into_iter()
            .find(|role| role.id == assignment.role_id)
            .map(Some)
            .ok_or(AdminPortError::LookupFailed)
    }

    async fn principal_assignment(
        &self,
        principal: &Principal,
    ) -> Result<Option<RoleAssignment>, AdminPortError> {
        let assignments = self.role_assignments().await?;
        let key = role_assignment_key(&principal.role, &principal.ref_);
        if let Some(assignment) = assignments.iter().find(|assignment| {
            role_assignment_key(&assignment.principal_role, &assignment.principal_ref) == key
        }) {
            return Ok(Some(assignment.clone()));
        }
        if principal.role == PrincipalRole::Aparatchi {
            let fallback_key = role_assignment_key(&PrincipalRole::Customer, &principal.ref_);
            return Ok(assignments.into_iter().find(|assignment| {
                role_assignment_key(&assignment.principal_role, &assignment.principal_ref)
                    == fallback_key
            }));
        }
        Ok(None)
    }

    async fn ensure_system_role_assignment_uses_native_principal(
        &self,
        assignment: &RoleAssignment,
    ) -> Result<(), AdminPortError> {
        if assignment.role_id != "material_taminotchi"
            || assignment.principal_role != PrincipalRole::MaterialTaminotchi
        {
            return Ok(());
        }
        let ref_key = assignment.principal_ref.trim().to_ascii_uppercase();
        if ref_key.starts_with("CUST") || ref_key.starts_with("CUS-") || ref_key.starts_with("SUP")
        {
            return Err(AdminPortError::InvalidInput(
                "material_taminotchi role cannot be assigned to another system directory ref"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

fn expand_item_groups_with_descendants(
    groups: Vec<String>,
    tree: &[crate::core::admin::models::AdminItemGroup],
) -> Vec<String> {
    let mut scope = Vec::new();
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::new();
    for group in groups {
        push_item_group_scope(&group, &mut scope, &mut seen, &mut queue);
    }
    while let Some(parent) = queue.pop_front() {
        for group in tree {
            if !group
                .parent_item_group
                .trim()
                .eq_ignore_ascii_case(parent.trim())
            {
                continue;
            }
            let name = group.item_group_name.trim();
            let name = if name.is_empty() {
                group.name.trim()
            } else {
                name
            };
            push_item_group_scope(name, &mut scope, &mut seen, &mut queue);
        }
    }
    scope
}

fn push_item_group_scope(
    group: &str,
    scope: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
    queue: &mut VecDeque<String>,
) {
    let group = group.trim();
    if group.is_empty() {
        return;
    }
    if seen.insert(group.to_ascii_lowercase()) {
        scope.push(group.to_string());
        queue.push_back(group.to_string());
    }
}
