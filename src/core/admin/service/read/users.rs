use super::super::helpers::*;
use super::super::*;

use std::collections::{BTreeMap, BTreeSet};

impl AdminService {
    pub async fn user_list_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<AdminUserListPage, AdminPortError> {
        let fetch_limit = offset.saturating_add(limit).saturating_add(1);
        let settings = self.settings().await?;
        let roles = self.role_definitions().await?;
        let assignments = self.role_assignments().await?;
        let role_labels = role_label_lookup(&roles, &assignments);
        let states = self.states().await?;
        let read = self.read_port()?;
        let suppliers = read.suppliers_page(query, fetch_limit, 0).await?;
        let customers = read.customers_page(query, fetch_limit, 0).await?;
        let normalized_query = normalize_search(query);
        let mut entries = Vec::new();
        let mut seen_ids = BTreeSet::new();

        for assignment in assignments.iter().filter(|assignment| {
            assignment.principal_role == PrincipalRole::Qolipchi
                && assignment.role_id.trim() == "qolipchi"
        }) {
            let customer_ref = assignment.principal_ref.trim();
            if customer_ref.is_empty() {
                continue;
            }
            let customers = read.customers_page(customer_ref, 10, 0).await?;
            let Some(customer) = customers
                .into_iter()
                .find(|customer| customer.ref_.trim() == customer_ref)
            else {
                continue;
            };
            let state = states
                .get(customer.ref_.trim())
                .cloned()
                .unwrap_or_default();
            if state.removed {
                continue;
            }
            let entry = customer_directory_entry(customer);
            let role_label = role_labels
                .get(&role_assignment_key(&PrincipalRole::Qolipchi, &entry.ref_))
                .cloned()
                .unwrap_or_else(|| "Qolipchi".to_string());
            let entry = AdminUserListEntry {
                id: format!("qolipchi:{}", entry.ref_),
                source: "qolipchi".to_string(),
                entity_ref: entry.ref_,
                principal_role: PrincipalRole::Qolipchi,
                name: entry.name,
                phone: entry.phone,
                role_label,
                blocked: false,
                status: "active".to_string(),
            };
            if user_list_matches(&entry, &normalized_query) && seen_ids.insert(entry.id.clone()) {
                entries.push(entry);
            }
        }

        if let Some(entry) = werka_user_list_entry(&settings, &role_labels) {
            if user_list_matches(&entry, &normalized_query) && seen_ids.insert(entry.id.clone()) {
                entries.push(entry);
            }
        }

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
                role_label,
                blocked: supplier.blocked,
                status: if supplier.blocked {
                    "blocked".to_string()
                } else {
                    "active".to_string()
                },
            };
            if user_list_matches(&entry, &normalized_query) && seen_ids.insert(entry.id.clone()) {
                entries.push(entry);
            }
        }

        for customer in customers {
            let state = states
                .get(customer.ref_.trim())
                .cloned()
                .unwrap_or_default();
            if state.removed {
                continue;
            }
            let entry = customer_directory_entry(customer);
            let principal_role = customer_user_principal_role(&assignments, &entry.ref_);
            let role_label = role_labels
                .get(&role_assignment_key(&principal_role, &entry.ref_))
                .cloned()
                .unwrap_or_else(|| user_list_default_role_label(&principal_role).to_string());
            let source = user_list_source(&principal_role);
            let entry = AdminUserListEntry {
                id: format!("{}:{}", source, entry.ref_),
                source: source.to_string(),
                entity_ref: entry.ref_,
                principal_role,
                name: entry.name,
                phone: entry.phone,
                role_label,
                blocked: false,
                status: "active".to_string(),
            };
            if user_list_matches(&entry, &normalized_query) && seen_ids.insert(entry.id.clone()) {
                entries.push(entry);
            }
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
        let now = OffsetDateTime::now_utc();
        Ok(AdminWorkerDetail {
            id: worker.id,
            name: worker.name,
            phone: worker.phone,
            level: worker.level,
            code: state.custom_code.trim().to_string(),
            code_locked: state.code_locked(now),
            code_retry_after_sec: state.retry_after_seconds(now),
        })
    }

    pub async fn activity(&self, items: AdminActivity) -> Result<AdminActivity, AdminPortError> {
        Ok(items.into_iter().take(30).collect())
    }
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
        role_label: role_labels
            .get(&role_assignment_key(&PrincipalRole::Werka, "werka"))
            .cloned()
            .unwrap_or_else(|| "Werka".to_string()),
        blocked: false,
        status: "active".to_string(),
    })
}

fn customer_user_principal_role(assignments: &[RoleAssignment], ref_: &str) -> PrincipalRole {
    let ref_ = ref_.trim();
    if assignments.iter().any(|assignment| {
        assignment.principal_ref.trim() == ref_
            && assignment.principal_role == PrincipalRole::Qolipchi
            && assignment.role_id.trim() == "qolipchi"
    }) {
        PrincipalRole::Qolipchi
    } else {
        PrincipalRole::Customer
    }
}

fn user_list_source(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Qolipchi => "qolipchi",
        _ => "customer",
    }
}

fn user_list_default_role_label(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Qolipchi => "Qolipchi",
        _ => "Customer",
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
