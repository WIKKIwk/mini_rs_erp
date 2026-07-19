use super::super::helpers::*;
use super::super::*;

impl AdminService {
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
        let avatar_url = self.profile_avatar_url("customer", &entry.ref_).await;
        let now = OffsetDateTime::now_utc();
        Ok(AdminCustomerDetail {
            ref_: entry.ref_,
            name: entry.name,
            phone: entry.phone,
            avatar_url,
            code: state.custom_code.trim().to_string(),
            code_locked: state.code_locked(now),
            code_retry_after_sec: state.retry_after_seconds(now),
            assigned_items,
            assigned_item_groups: Vec::new(),
            assigned_warehouses: Vec::new(),
        })
    }
}
