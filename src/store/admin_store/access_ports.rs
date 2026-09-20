use async_trait::async_trait;

use super::*;

#[async_trait]
impl AdminStatePort for JsonAdminStore {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(data
            .states
            .iter()
            .map(|(key, state)| (key.clone(), AdminState::from(state)))
            .collect())
    }

    async fn put_state(&self, ref_: &str, mut state: AdminState) -> Result<(), AdminPortError> {
        if !state.custom_code.is_empty() && !crate::core::auth::password::is_password_hash(&state.custom_code) {
            state.custom_code = crate::core::auth::password::hash_password(state.custom_code)
                .await.map_err(|_| AdminPortError::LookupFailed)?;
        }
        state.pending_persist_code.clear();
        state.pending_persist_at = None;
        let mut data = self.data.lock().await;
        data.states
            .insert(ref_.trim().to_string(), StoredAdminState::from(&state));
        self.persist(&data).await
    }
}

#[async_trait]
impl SupplierLookup for JsonAdminStore {
    async fn search_suppliers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierRecord>, AuthPortError> {
        let data = self.data.lock().await;
        Ok(paginate(
            data.suppliers
                .values()
                .filter(|entry| entry_matches(entry, query))
                .map(SupplierRecord::from)
                .collect(),
            limit,
            0,
        ))
    }
}

#[async_trait]
impl CustomerLookup for JsonAdminStore {
    async fn search_customers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CustomerRecord>, AuthPortError> {
        let data = self.data.lock().await;
        Ok(paginate(
            data.customers
                .values()
                .filter(|entry| entry_matches(entry, query))
                .map(CustomerRecord::from)
                .collect(),
            limit,
            0,
        ))
    }
}

#[async_trait]
impl MaterialTaminotchiLookup for JsonAdminStore {
    async fn search_material_taminotchilar(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MaterialTaminotchiRecord>, AuthPortError> {
        let data = self.data.lock().await;
        Ok(paginate(
            data.material_taminotchilar
                .values()
                .filter(|entry| entry_matches(entry, query))
                .map(MaterialTaminotchiRecord::from)
                .collect(),
            limit,
            0,
        ))
    }
}

#[async_trait]
impl AdminAccessStateLookup for JsonAdminStore {
    async fn list_states(&self) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError> {
        let data = self.data.lock().await;
        Ok(data
            .states
            .iter()
            .map(|(key, state)| {
                (
                    key.clone(),
                    AdminAccessState {
                        custom_code: state.custom_code.clone(),
                        blocked: state.blocked,
                        removed: state.removed,
                    },
                )
            })
            .collect())
    }
}
