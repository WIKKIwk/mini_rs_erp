use super::super::helpers::*;
use super::super::*;

use std::collections::{BTreeMap, BTreeSet};

impl AdminService {
    pub async fn user_list_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
        role_filter: Option<&str>,
    ) -> Result<AdminUserListPage, AdminPortError> {
        let settings = self.settings().await?;
        let roles = self.role_definitions().await?;
        let assignments = self.role_assignments().await?;
        let role_labels = role_label_lookup(&roles, &assignments);
        let states = self.states().await?;
        let read = self.read_port()?;
        let normalized_query = normalize_search(query);
        let role_filter = normalize_user_role_filter(role_filter);
        let mut entries = Vec::new();
        let mut seen_ids = BTreeSet::new();

        if (role_filter.is_none() || role_filter == Some("werka"))
            && let Some(entry) = werka_user_list_entry(&settings, &role_labels)
            && user_list_matches(&entry, &normalized_query)
            && seen_ids.insert(entry.id.clone())
        {
            entries.push(entry);
        }

        if role_filter.is_none() || role_filter == Some("supplier") {
            let suppliers = read
                .suppliers_page(query, scan_limit(limit, offset), 0)
                .await?;
            for supplier in self.admin_suppliers_from_entries(suppliers, &states)? {
                let role_label = role_labels
                    .get(&role_assignment_key(
                        &PrincipalRole::Supplier,
                        &supplier.ref_,
                    ))
                    .cloned()
                    .unwrap_or_else(|| "Supplier".to_string());
                let entry = AdminUserListEntry {
                    id: format!("supplier:{}", supplier.ref_),
                    source: "supplier".to_string(),
                    entity_ref: supplier.ref_,
                    principal_role: PrincipalRole::Supplier,
                    name: supplier.name,
                    phone: supplier.phone,
                    avatar_url: String::new(),
                    role_label,
                    blocked: supplier.blocked,
                    status: if supplier.blocked {
                        "blocked".to_string()
                    } else {
                        "active".to_string()
                    },
                };
                if user_list_matches(&entry, &normalized_query) && seen_ids.insert(entry.id.clone())
                {
                    entries.push(entry);
                }
            }
        }

        if role_filter.is_none() || role_filter == Some("customer") {
            let customers = read
                .customers_page(query, scan_limit(limit, offset), 0)
                .await?;
            for customer in customers {
                let entry = customer_directory_entry(customer);
                if material_assignment_for_ref(&assignments, &entry.ref_) {
                    continue;
                }
                let Some(entry) =
                    customer_user_list_entry(entry, PrincipalRole::Customer, &role_labels, &states)
                else {
                    continue;
                };
                if user_list_matches(&entry, &normalized_query) && seen_ids.insert(entry.id.clone())
                {
                    entries.push(entry);
                }
            }
        }

        if role_filter.is_none() || role_filter == Some("material_taminotchi") {
            let materials = read
                .material_taminotchilar_page(query, scan_limit(limit, offset), 0)
                .await?;
            for material in materials {
                let Some(entry) = material_taminotchi_user_list_entry(
                    customer_directory_entry(material),
                    &role_labels,
                    &states,
                ) else {
                    continue;
                };
                if user_list_matches(&entry, &normalized_query) && seen_ids.insert(entry.id.clone())
                {
                    entries.push(entry);
                }
            }
        }

        for entry in &mut entries {
            entry.avatar_url = self
                .profile_avatar_url(profile_role_key(&entry.principal_role), &entry.entity_ref)
                .await;
        }

        let has_more = entries.len() > offset.saturating_add(limit);
        let items = entries.into_iter().skip(offset).take(limit).collect();
        Ok(AdminUserListPage { items, has_more })
    }

    pub async fn worker_detail(&self, worker: Worker) -> Result<AdminWorkerDetail, AdminPortError> {
        let state = self.state_for(&worker.id).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        let avatar_url = self.profile_avatar_url("worker", &worker.id).await;
        let now = OffsetDateTime::now_utc();
        Ok(AdminWorkerDetail {
            id: worker.id,
            name: worker.name,
            phone: worker.phone,
            avatar_url,
            level: worker.level,
            code: state.custom_code.trim().to_string(),
            code_locked: state.code_locked(now),
            code_retry_after_sec: state.retry_after_seconds(now),
        })
    }

    pub async fn system_user_detail(
        &self,
        user: SystemUser,
    ) -> Result<AdminSystemUserDetail, AdminPortError> {
        let state = self.state_for(&user.id).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        let avatar_url = self
            .profile_avatar_url(profile_role_key(&user.role), &user.id)
            .await;
        let now = OffsetDateTime::now_utc();
        Ok(AdminSystemUserDetail {
            id: user.id,
            role: user.role,
            name: user.name,
            phone: user.phone,
            avatar_url,
            code: state.custom_code.trim().to_string(),
            blocked: state.blocked,
            code_locked: state.code_locked(now),
            code_retry_after_sec: state.retry_after_seconds(now),
        })
    }

    pub async fn activity(&self, items: AdminActivity) -> Result<AdminActivity, AdminPortError> {
        Ok(items.into_iter().take(30).collect())
    }
}

fn normalize_user_role_filter(value: Option<&str>) -> Option<&'static str> {
    match value
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" => None,
        "werka" | "omborchi" => Some("werka"),
        "supplier" | "taminotchi" | "ta'minotchi" | "ta’minotchi" => Some("supplier"),
        "customer" | "haridor" => Some("customer"),
        "material_taminotchi" | "material-taminotchi" | "materialtaminotchi" => {
            Some("material_taminotchi")
        }
        "worker" | "ishchi" => Some("worker"),
        "qolipchi" => Some("qolipchi"),
        "boyoqchi" | "bo'yoqchi" | "bo‘yoqchi" => Some("boyoqchi"),
        _ => Some("__unknown__"),
    }
}

fn scan_limit(limit: usize, offset: usize) -> usize {
    offset.saturating_add(limit).saturating_add(1).max(50)
}

fn material_assignment_for_ref(assignments: &[RoleAssignment], ref_: &str) -> bool {
    let ref_ = ref_.trim();
    assignments.iter().any(|assignment| {
        assignment.principal_ref.trim() == ref_
            && (assignment.principal_role == PrincipalRole::MaterialTaminotchi
                || assignment.role_id == "material_taminotchi")
    })
}

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
    settings: &AdminSettings,
    role_labels: &BTreeMap<String, String>,
) -> Option<AdminUserListEntry> {
    if settings.werka_name.trim().is_empty() && settings.werka_phone.trim().is_empty() {
        return None;
    }
    Some(AdminUserListEntry {
        id: "werka:werka".to_string(),
        source: "werka".to_string(),
        entity_ref: "werka".to_string(),
        principal_role: PrincipalRole::Werka,
        name: if settings.werka_name.trim().is_empty() {
            "Werka".to_string()
        } else {
            settings.werka_name.trim().to_string()
        },
        phone: settings.werka_phone.trim().to_string(),
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
