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
        let code = (!state.custom_code.is_empty() && !crate::core::auth::password::is_password_hash(&state.custom_code))
            .then(|| state.custom_code.clone());
        if let Some(code) = &code {
            state.custom_code = crate::core::auth::password::hash_password(code.clone())
                .await.map_err(|_| AdminPortError::LookupFailed)?;
        }
        state.pending_persist_code.clear();
        state.pending_persist_at = None;
        let mut data = self.data.lock().await;
        let mut stored = StoredAdminState::from(&state);
        if let Some(code) = code {
            stored.encrypted_code = crate::core::auth::code_vault::CodeCipher::load()
                .and_then(|cipher| cipher.encrypt(ref_.trim(), &stored.custom_code, &code))
                .map_err(|_| AdminPortError::LookupFailed)?;
        } else if let Some(current) = data.states.get(ref_.trim()) {
            stored.custom_code = current.custom_code.clone();
            stored.encrypted_code = current.encrypted_code.clone();
        }
        data.states.insert(ref_.trim().to_string(), stored);
        self.persist(&data).await
    }

    async fn access_code(&self, ref_: &str) -> Result<String, AdminPortError> {
        let data = self.data.lock().await;
        let Some(state) = data.states.get(ref_.trim()) else { return Ok(String::new()); };
        if state.encrypted_code.is_empty() {
            return Ok(if crate::core::auth::password::is_password_hash(&state.custom_code) {
                String::new()
            } else { state.custom_code.clone() });
        }
        crate::core::auth::code_vault::CodeCipher::load()
            .and_then(|cipher| cipher.decrypt(ref_.trim(), &state.custom_code, &state.encrypted_code))
            .map_err(|_| AdminPortError::LookupFailed)
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
