use super::helpers::*;
use super::*;
use crate::core::admin::models::AdminItemGroup;

impl AdminService {
    pub async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminSupplier, AdminPortError> {
        let entry = self
            .write_port()?
            .create_supplier(name.trim(), phone.trim())
            .await?;
        let mut state = self.state_for(&entry.ref_).await?;
        if state.removed {
            state.removed = false;
            state.blocked = false;
            self.put_state(&entry.ref_, state.clone()).await?;
        }
        self.build_supplier(entry, state)
    }

    pub async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<CustomerDirectoryEntry, AdminPortError> {
        let normalized = normalize_admin_phone(phone)?;
        for query in phone_search_terms(phone, &normalized) {
            let existing = self.read_port()?.customers_page(&query, 50, 0).await?;
            if existing
                .iter()
                .any(|entry| phone_matches(&entry.phone, &normalized))
            {
                return Err(AdminPortError::InvalidInput(
                    "phone already exists".to_string(),
                ));
            }
        }
        self.write_port()?
            .create_customer(name.trim(), &normalized)
            .await
            .map(customer_directory_entry)
    }

    pub async fn set_supplier_blocked(
        &self,
        ref_: &str,
        blocked: bool,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let (entry, mut state) = self.supplier_entry_state(ref_, false).await?;
        state.blocked = blocked;
        self.put_state(&entry.ref_, state).await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn update_supplier_phone(
        &self,
        ref_: &str,
        phone: &str,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let (entry, _) = self.supplier_entry_state(ref_, false).await?;
        let normalized = normalize_admin_phone(phone)?;
        self.write_port()?
            .update_supplier_phone(&entry.ref_, &normalized)
            .await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn update_customer_phone(
        &self,
        ref_: &str,
        phone: &str,
    ) -> Result<AdminCustomerDetail, AdminPortError> {
        let normalized = normalize_admin_phone(phone)?;
        self.write_port()?
            .update_customer_phone(ref_.trim(), &normalized)
            .await?;
        self.customer_detail(ref_).await
    }

    pub async fn update_supplier_items(
        &self,
        ref_: &str,
        item_codes: Vec<String>,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let (entry, _) = self.supplier_entry_state(ref_, false).await?;
        let normalized = normalize_item_codes(item_codes);
        if !normalized.is_empty() {
            let found = self.read_port()?.items_by_codes(&normalized).await?;
            for code in &normalized {
                if !found
                    .iter()
                    .any(|item| item.code.trim().eq_ignore_ascii_case(code.trim()))
                {
                    return Err(AdminPortError::InvalidInput(format!(
                        "item topilmadi: {code}"
                    )));
                }
            }
        }
        let current = self
            .read_port()?
            .assigned_supplier_items(&entry.ref_, 200)
            .await?
            .into_iter()
            .map(|item| item.code)
            .collect::<Vec<_>>();
        for code in &normalized {
            if !current
                .iter()
                .any(|current| current.trim().eq_ignore_ascii_case(code))
            {
                self.write_port()?
                    .assign_supplier_item(&entry.ref_, code)
                    .await?;
            }
        }
        for code in current {
            if !normalized
                .iter()
                .any(|desired| desired.trim().eq_ignore_ascii_case(code.trim()))
            {
                self.write_port()?
                    .unassign_supplier_item(&entry.ref_, &code)
                    .await?;
            }
        }
        let mut state = self.state_for(&entry.ref_).await?;
        state.assignments_configured = true;
        state.assigned_item_codes = normalized;
        self.put_state(&entry.ref_, state).await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let (entry, _) = self.supplier_entry_state(ref_, false).await?;
        let code = item_code.trim();
        self.write_port()?
            .assign_supplier_item(&entry.ref_, code)
            .await?;
        let mut state = self.state_for(&entry.ref_).await?;
        state.assignments_configured = true;
        state.assigned_item_codes = normalize_item_codes(
            state
                .assigned_item_codes
                .into_iter()
                .chain(std::iter::once(code.to_string()))
                .collect(),
        );
        self.put_state(&entry.ref_, state).await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let (entry, _) = self.supplier_entry_state(ref_, false).await?;
        self.write_port()?
            .unassign_supplier_item(&entry.ref_, item_code.trim())
            .await?;
        let mut state = self.state_for(&entry.ref_).await?;
        state.assignments_configured = true;
        state
            .assigned_item_codes
            .retain(|code| !code.trim().eq_ignore_ascii_case(item_code.trim()));
        self.put_state(&entry.ref_, state).await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<AdminCustomerDetail, AdminPortError> {
        let entry = self.read_port()?.customer_by_ref(ref_.trim()).await?;
        let state = self.state_for(&entry.ref_).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        self.write_port()?
            .assign_customer_item(&entry.ref_, item_code.trim())
            .await?;
        self.customer_detail(&entry.ref_).await
    }

    pub async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<AdminCustomerDetail, AdminPortError> {
        let entry = self.read_port()?.customer_by_ref(ref_.trim()).await?;
        let state = self.state_for(&entry.ref_).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        self.write_port()?
            .unassign_customer_item(&entry.ref_, item_code.trim())
            .await?;
        self.customer_detail(&entry.ref_).await
    }

    pub async fn regenerate_supplier_code(
        &self,
        ref_: &str,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let (entry, mut state) = self.supplier_entry_state(ref_, false).await?;
        let mut existing = self.existing_codes().await?;
        let now = OffsetDateTime::now_utc();
        state = bump_code_regen_state(state, now)?;
        state.custom_code = random_code(&self.config.read().await.supplier_prefix, &mut existing);
        state.pending_persist_code = state.custom_code.clone();
        state.pending_persist_at = Some(now + time::Duration::seconds(CODE_REGEN_WINDOW_SECONDS));
        self.put_state(&entry.ref_, state).await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn regenerate_customer_code(
        &self,
        ref_: &str,
    ) -> Result<AdminCustomerDetail, AdminPortError> {
        let entry = self.read_port()?.customer_by_ref(ref_.trim()).await?;
        let mut existing = self.existing_state_codes().await?;
        let mut state = self.state_for(&entry.ref_).await?;
        let now = OffsetDateTime::now_utc();
        state = bump_code_regen_state(state, now)?;
        let prefix = self.customer_access_code_prefix(&entry.ref_).await?;
        state.custom_code = random_code(&prefix, &mut existing);
        self.put_state(&entry.ref_, state.clone()).await?;
        self.write_port()?
            .update_customer_code(&entry.ref_, &state.custom_code)
            .await?;
        self.customer_detail(&entry.ref_).await
    }

    async fn customer_access_code_prefix(&self, ref_: &str) -> Result<String, AdminPortError> {
        let assignments = self.role_assignments().await?;
        let ref_ = ref_.trim();
        if assignments.iter().any(|assignment| {
            assignment.role_id == "aparatchi" && assignment.principal_ref.trim() == ref_
        }) {
            Ok("40".to_string())
        } else {
            Ok("30".to_string())
        }
    }

    pub async fn remove_supplier(&self, ref_: &str) -> Result<(), AdminPortError> {
        let (entry, mut state) = self.supplier_entry_state(ref_, false).await?;
        state.removed = true;
        state.blocked = true;
        self.put_state(&entry.ref_, state).await
    }

    pub async fn restore_supplier(
        &self,
        ref_: &str,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let (entry, mut state) = self.supplier_entry_state(ref_, true).await?;
        state.removed = false;
        state.blocked = false;
        self.put_state(&entry.ref_, state).await?;
        self.supplier_detail(&entry.ref_).await
    }

    pub async fn remove_customer(&self, ref_: &str) -> Result<(), AdminPortError> {
        let entry = self.read_port()?.customer_by_ref(ref_.trim()).await?;
        let mut state = self.state_for(&entry.ref_).await?;
        state.removed = true;
        state.blocked = true;
        self.put_state(&entry.ref_, state).await
    }

    pub async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        self.write_port()?
            .create_item(code.trim(), name.trim(), uom.trim(), item_group.trim())
            .await
    }

    pub async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item group name is required".to_string(),
            ));
        }
        let parent = if parent.trim().is_empty() {
            "All Item Groups"
        } else {
            parent.trim()
        };
        self.write_port()?
            .create_item_group(name, parent, is_group)
            .await
    }

    pub async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item group name is required".to_string(),
            ));
        }
        if name == "All Item Groups" {
            return Err(AdminPortError::InvalidInput(
                "root item group cannot be moved".to_string(),
            ));
        }
        let parent = if parent.trim().is_empty() {
            "All Item Groups"
        } else {
            parent.trim()
        };
        if name == parent {
            return Err(AdminPortError::InvalidInput(
                "item group cannot be its own parent".to_string(),
            ));
        }
        self.write_port()?
            .move_item_group_parent(name, parent)
            .await
    }

    pub async fn move_items_to_group(
        &self,
        item_codes: Vec<String>,
        item_group: &str,
    ) -> Result<AdminItemGroupBulkMoveResult, AdminPortError> {
        let codes = normalize_item_codes(item_codes);
        if codes.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item codes are required".to_string(),
            ));
        }
        let group = item_group.trim();
        if group.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item group is required".to_string(),
            ));
        }
        let mut updated = Vec::new();
        let mut failed = Vec::new();
        for code in &codes {
            if self
                .write_port()?
                .update_item_group(code, group)
                .await
                .is_ok()
            {
                updated.push(code.clone());
            } else {
                failed.push(code.clone());
            }
        }
        Ok(AdminItemGroupBulkMoveResult {
            item_group: group.to_string(),
            requested_count: codes.len(),
            updated_count: updated.len(),
            failed_count: failed.len(),
            updated_item_codes: updated,
            failed_item_codes: failed,
        })
    }

    pub async fn regenerate_werka_code(&self) -> Result<AdminSettings, AdminPortError> {
        let mut state = self.state_for("werka").await?;
        let now = OffsetDateTime::now_utc();
        state = bump_code_regen_state(state, now)?;
        let mut existing = BTreeMap::new();
        let code = random_code(&self.config.read().await.werka_prefix, &mut existing);
        state.custom_code = code.clone();
        self.put_state("werka", state).await?;
        self.config.write().await.werka_code = code;
        let config = self.config.read().await;
        self.update_auth_runtime(
            &config.werka_phone,
            &config.werka_code,
            &config.werka_name,
            &config.admin_phone,
            &config.admin_name,
        );
        drop(config);
        if let Some(persister) = &self.env_persister {
            persister.upsert(BTreeMap::from([(
                "MOBILE_DEV_WERKA_CODE",
                self.config.read().await.werka_code.clone(),
            )]))?;
        }
        self.settings().await
    }
}
