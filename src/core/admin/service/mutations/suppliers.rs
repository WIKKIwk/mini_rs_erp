use super::super::helpers::*;
use super::super::*;

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
}
