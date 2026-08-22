
fn customer_user_list_entry(
    entry: CustomerDirectoryEntry,
    principal_role: PrincipalRole,
    role_labels: &BTreeMap<String, String>,
    states: &BTreeMap<String, AdminState>,
) -> Option<AdminUserListEntry> {
    let state = states.get(entry.ref_.trim()).cloned().unwrap_or_default();
    if state.removed {
        return None;
    }
    let role_label = role_labels
        .get(&role_assignment_key(&principal_role, &entry.ref_))
        .cloned()
        .unwrap_or_else(|| match principal_role {
            PrincipalRole::MaterialTaminotchi => "Material taminotchisi".to_string(),
            _ => "Customer".to_string(),
        });
    Some(AdminUserListEntry {
        id: format!("customer:{}", entry.ref_),
        source: "customer".to_string(),
        entity_ref: entry.ref_,
        principal_role,
        name: entry.name,
        phone: entry.phone,
        avatar_url: String::new(),
        role_label,
        blocked: false,
        status: "active".to_string(),
    })
}

fn material_taminotchi_user_list_entry(
    entry: CustomerDirectoryEntry,
    role_labels: &BTreeMap<String, String>,
    states: &BTreeMap<String, AdminState>,
) -> Option<AdminUserListEntry> {
    let state = states.get(entry.ref_.trim()).cloned().unwrap_or_default();
    if state.removed {
        return None;
    }
    let role_label = role_labels
        .get(&role_assignment_key(
            &PrincipalRole::MaterialTaminotchi,
            &entry.ref_,
        ))
        .cloned()
        .unwrap_or_else(|| "Material taminotchisi".to_string());
    Some(AdminUserListEntry {
        id: format!("material_taminotchi:{}", entry.ref_),
        source: "material_taminotchi".to_string(),
        entity_ref: entry.ref_,
        principal_role: PrincipalRole::MaterialTaminotchi,
        name: entry.name,
        phone: entry.phone,
        avatar_url: String::new(),
        role_label,
        blocked: state.blocked,
        status: if state.blocked {
            "blocked".to_string()
        } else {
            "active".to_string()
        },
    })
}

fn role_label_lookup(
    roles: &[RoleDefinition],
    assignments: &[RoleAssignment],
) -> BTreeMap<String, String> {
    let labels = roles
        .iter()
        .map(|role| (role.id.as_str(), role.label.trim()))
        .collect::<BTreeMap<_, _>>();
    assignments
        .iter()
        .filter_map(|assignment| {
            labels.get(assignment.role_id.as_str()).map(|label| {
                (
                    role_assignment_key(&assignment.principal_role, &assignment.principal_ref),
                    (*label).to_string(),
                )
            })
        })
        .collect()
}

fn werka_user_list_entry(
    werka_name: &str,
    werka_phone: &str,
    role_labels: &BTreeMap<String, String>,
) -> Option<AdminUserListEntry> {
    if werka_name.trim().is_empty() && werka_phone.trim().is_empty() {
        return None;
    }
    Some(AdminUserListEntry {
        id: "werka:werka".to_string(),
        source: "werka".to_string(),
        entity_ref: "werka".to_string(),
        principal_role: PrincipalRole::Werka,
        name: if werka_name.trim().is_empty() {
            "Werka".to_string()
        } else {
            werka_name.trim().to_string()
        },
        phone: werka_phone.trim().to_string(),
        avatar_url: String::new(),
        role_label: role_labels
            .get(&role_assignment_key(&PrincipalRole::Werka, "werka"))
            .cloned()
            .unwrap_or_else(|| "Werka".to_string()),
        blocked: false,
        status: "active".to_string(),
    })
}

fn profile_role_key(role: &PrincipalRole) -> &'static str {
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

fn normalize_search(value: &str) -> String {
    value.trim().to_lowercase()
}

fn user_list_matches(entry: &AdminUserListEntry, query: &str) -> bool {
    query.is_empty()
        || entry.name.to_lowercase().contains(query)
        || entry.phone.to_lowercase().contains(query)
        || entry.entity_ref.to_lowercase().contains(query)
        || entry.role_label.to_lowercase().contains(query)
        || entry.source.to_lowercase().contains(query)
}
