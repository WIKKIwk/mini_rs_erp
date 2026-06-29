use super::super::*;

impl AdminService {
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
        let avatar_url = self.profile_avatar_url("supplier", &entry.ref_).await;
        let now = OffsetDateTime::now_utc();
        Ok(AdminSupplierDetail {
            ref_: entry.ref_,
            name: entry.name,
            phone: entry.phone,
            avatar_url,
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
}
