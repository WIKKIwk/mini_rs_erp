use super::helpers::*;
use super::*;

use crate::core::admin::models::{AdminItemGroup, AdminWarehouse};

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

        if let Some(entry) = werka_user_list_entry(&settings, &role_labels) {
            if user_list_matches(&entry, &normalized_query) {
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
            if user_list_matches(&entry, &normalized_query) {
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
            let role_label = role_labels
                .get(&role_assignment_key(&PrincipalRole::Customer, &entry.ref_))
                .cloned()
                .unwrap_or_else(|| "Customer".to_string());
            let entry = AdminUserListEntry {
                id: format!("customer:{}", entry.ref_),
                source: "customer".to_string(),
                entity_ref: entry.ref_,
                principal_role: PrincipalRole::Customer,
                name: entry.name,
                phone: entry.phone,
                role_label,
                blocked: false,
                status: "active".to_string(),
            };
            if user_list_matches(&entry, &normalized_query) {
                entries.push(entry);
            }
        }

        let has_more = entries.len() > offset.saturating_add(limit);
        let items = entries.into_iter().skip(offset).take(limit).collect();
        Ok(AdminUserListPage { items, has_more })
    }

    pub async fn suppliers_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminSupplier>, AdminPortError> {
        let states = self.states().await?;
        let entries = self.read_port()?.suppliers_page("", limit, offset).await?;
        self.admin_suppliers_from_entries(entries, &states)
    }

    pub async fn suppliers(&self, limit: usize) -> Result<Vec<AdminSupplier>, AdminPortError> {
        let states = self.states().await?;
        let entries = self.supplier_entries(limit).await?;
        self.admin_suppliers_from_entries(entries, &states)
    }

    pub async fn supplier_summary(
        &self,
        _limit: usize,
    ) -> Result<AdminSupplierSummary, AdminPortError> {
        let states = self.states().await?;
        let entries = self.supplier_entries(0).await?;
        let mut summary = AdminSupplierSummary {
            total_suppliers: entries.len(),
            ..AdminSupplierSummary::default()
        };
        for entry in entries {
            let state = states.get(entry.ref_.trim()).cloned().unwrap_or_default();
            if state.blocked || state.removed {
                summary.blocked_suppliers += 1;
            } else {
                summary.active_suppliers += 1;
            }
        }
        Ok(summary)
    }

    pub async fn inactive_suppliers(
        &self,
        limit: usize,
    ) -> Result<Vec<AdminSupplier>, AdminPortError> {
        let states = self.states().await?;
        let entries = self.supplier_entries(limit).await?;
        let mut result = Vec::new();
        for entry in entries {
            let state = states.get(entry.ref_.trim()).cloned().unwrap_or_default();
            if !state.blocked && !state.removed {
                continue;
            }
            result.push(self.build_supplier(entry, state)?);
        }
        Ok(result)
    }

    pub async fn supplier_detail(&self, ref_: &str) -> Result<AdminSupplierDetail, AdminPortError> {
        let (entry, state) = self.supplier_entry_state(ref_, false).await?;
        let read = self.read_port()?;
        let assigned_items = match read.assigned_supplier_items(&entry.ref_, 200).await {
            Ok(items) => items,
            #[cfg(test)]
            Err(AdminPortError::PermissionDenied) => {
                if state.assigned_item_codes.is_empty() {
                    Vec::new()
                } else {
                    read.items_by_codes(&state.assigned_item_codes).await?
                }
            }
            Err(err) => return Err(err),
        };
        let code = self.supplier_code(&entry, &state)?;
        let now = OffsetDateTime::now_utc();
        Ok(AdminSupplierDetail {
            ref_: entry.ref_,
            name: entry.name,
            phone: entry.phone,
            code,
            blocked: state.blocked,
            removed: state.removed,
            code_locked: state.code_locked(now),
            code_retry_after_sec: state.retry_after_seconds(now),
            assigned_items,
        })
    }

    pub async fn assigned_supplier_items(
        &self,
        ref_: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let (entry, _state) = self.supplier_entry_state(ref_, false).await?;
        let read = self.read_port()?;
        match read.assigned_supplier_items(&entry.ref_, limit).await {
            Ok(items) => Ok(items),
            #[cfg(test)]
            Err(AdminPortError::PermissionDenied) if _state.assigned_item_codes.is_empty() => {
                Ok(Vec::new())
            }
            #[cfg(test)]
            Err(AdminPortError::PermissionDenied) => {
                read.items_by_codes(&_state.assigned_item_codes).await
            }
            Err(err) => Err(err),
        }
    }

    pub async fn customers(
        &self,
        limit: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, AdminPortError> {
        let read = self.read_port()?;
        let states = self.states().await?;
        let entries = read.customers_page("", limit, 0).await?;
        Ok(entries
            .into_iter()
            .filter(|entry| {
                !states
                    .get(entry.ref_.trim())
                    .map(|state| state.removed)
                    .unwrap_or(false)
            })
            .map(customer_directory_entry)
            .collect())
    }

    pub async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, AdminPortError> {
        let read = self.read_port()?;
        let states = self.states().await?;
        let entries = read.customers_page(query, limit, offset).await?;
        Ok(entries
            .into_iter()
            .filter(|entry| {
                !states
                    .get(entry.ref_.trim())
                    .map(|state| state.removed)
                    .unwrap_or(false)
            })
            .map(customer_directory_entry)
            .collect())
    }

    pub async fn customer_detail(&self, ref_: &str) -> Result<AdminCustomerDetail, AdminPortError> {
        let read = self.read_port()?;
        let entry = read.customer_by_ref(ref_.trim()).await?;
        let state = self.state_for(&entry.ref_).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        let assigned_items = read.customer_items(&entry.ref_, "", 200).await?;
        let now = OffsetDateTime::now_utc();
        Ok(AdminCustomerDetail {
            ref_: entry.ref_,
            name: entry.name,
            phone: entry.phone,
            code: state.custom_code.trim().to_string(),
            code_locked: state.code_locked(now),
            code_retry_after_sec: state.retry_after_seconds(now),
            assigned_items,
        })
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

    pub async fn items_page_by_group(
        &self,
        group: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.read_port()?
            .items_page_by_group(group, query, limit, offset)
            .await
    }

    pub async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        self.read_port()?.items_by_codes(item_codes).await
    }

    pub async fn item_groups(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<String>, AdminPortError> {
        let groups = self.read_port()?.item_groups(query, limit).await?;
        if groups.is_empty() && query.trim().is_empty() {
            Ok(vec!["All Item Groups".to_string()])
        } else {
            Ok(dedupe_strings(groups))
        }
    }

    pub async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, AdminPortError> {
        self.read_port()?
            .warehouses(query, normalize_warehouse_parent(parent), limit)
            .await
    }

    pub async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        let groups = self.read_port()?.item_group_tree().await?;
        if groups.is_empty() {
            Ok(vec![AdminItemGroup {
                name: "All Item Groups".to_string(),
                item_group_name: "All Item Groups".to_string(),
                parent_item_group: String::new(),
                is_group: true,
            }])
        } else {
            Ok(groups)
        }
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

fn normalize_warehouse_parent(parent: &str) -> &str {
    if parent.trim().eq_ignore_ascii_case("Aparat") {
        "aparat - A"
    } else {
        parent
    }
}
