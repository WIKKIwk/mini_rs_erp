use super::super::helpers::*;
use super::super::*;
use crate::core::admin::item_customer_policy::{
    FINISHED_GOODS_CUSTOMER_REQUIRED, item_group_requires_customer,
};

impl AdminService {
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
        let item_code = item_code.trim();
        let detail = self.item_detail(item_code).await?;
        let groups = self.item_group_tree().await?;
        let removes_existing_assignment = detail
            .customers
            .iter()
            .any(|customer| customer.ref_.trim().eq_ignore_ascii_case(&entry.ref_));
        if removes_existing_assignment
            && detail.customers.len() <= 1
            && item_group_requires_customer(&detail.item_group, &groups)
        {
            return Err(AdminPortError::InvalidInput(
                FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
            ));
        }
        self.write_port()?
            .unassign_customer_item_guarded(&entry.ref_, item_code)
            .await?;
        self.customer_detail(&entry.ref_).await
    }

    pub async fn remove_customer(&self, ref_: &str) -> Result<(), AdminPortError> {
        let entry = self.read_port()?.customer_by_ref(ref_.trim()).await?;
        let mut state = self.state_for(&entry.ref_).await?;
        state.removed = true;
        state.blocked = true;
        self.put_state(&entry.ref_, state).await
    }
}
