use super::super::helpers::*;
use super::super::*;

impl AdminService {
    pub async fn regenerate_supplier_code(
        &self,
        ref_: &str,
    ) -> Result<AdminSupplierDetail, AdminPortError> {
        let (entry, mut state) = self.supplier_entry_state(ref_, false).await?;
        let mut existing = self.existing_codes().await?;
        let now = OffsetDateTime::now_utc();
        state = bump_code_regen_state(state, now)?;
        state.custom_code = random_code(&self.config.read().await.supplier_prefix, &mut existing);
        let code = state.custom_code.clone();
        state.pending_persist_code.clear();
        state.pending_persist_at = None;
        self.put_state(&entry.ref_, state).await?;
        let mut detail = self.supplier_detail(&entry.ref_).await?;
        detail.code = code;
        Ok(detail)
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
        let code = state.custom_code.clone();
        self.put_state(&entry.ref_, state).await?;
        let mut detail = self.customer_detail(&entry.ref_).await?;
        detail.code = code;
        Ok(detail)
    }

    pub async fn regenerate_worker_code(
        &self,
        worker: Worker,
    ) -> Result<AdminWorkerDetail, AdminPortError> {
        let mut existing = self.existing_state_codes().await?;
        let mut state = self.state_for(&worker.id).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        let now = OffsetDateTime::now_utc();
        state = bump_code_regen_state(state, now)?;
        state.custom_code = random_code("40", &mut existing);
        let code = state.custom_code.clone();
        self.put_state(&worker.id, state).await?;
        let mut detail = self.worker_detail(worker).await?;
        detail.code = code;
        Ok(detail)
    }

    pub async fn regenerate_system_user_code(
        &self,
        user: SystemUser,
    ) -> Result<AdminSystemUserDetail, AdminPortError> {
        let mut existing = self.existing_state_codes().await?;
        let mut state = self.state_for(&user.id).await?;
        if state.removed || !matches!(user.role, PrincipalRole::Qolipchi | PrincipalRole::Boyoqchi | PrincipalRole::TayyorlovMasteri | PrincipalRole::HomashyoRezkachi)
        {
            return Err(AdminPortError::NotFound);
        }
        let now = OffsetDateTime::now_utc();
        state = bump_code_regen_state(state, now)?;
        let prefix = match user.role {
            PrincipalRole::Qolipchi => "50",
            PrincipalRole::Boyoqchi => "80",
            PrincipalRole::TayyorlovMasteri => "90",
            PrincipalRole::HomashyoRezkachi => "91",
            _ => return Err(AdminPortError::NotFound),
        };
        state.custom_code = random_code(prefix, &mut existing);
        let code = state.custom_code.clone();
        self.put_state(&user.id, state).await?;
        let mut detail = self.system_user_detail(user).await?;
        detail.code = code;
        Ok(detail)
    }

    async fn customer_access_code_prefix(&self, ref_: &str) -> Result<String, AdminPortError> {
        let assignments = self.role_assignments().await?;
        let ref_ = ref_.trim();
        if assignments.iter().any(|assignment| {
            assignment.role_id == "aparatchi" && assignment.principal_ref.trim() == ref_
        }) {
            Ok("40".to_string())
        } else if assignments.iter().any(|assignment| {
            assignment.principal_ref.trim() == ref_
                && (assignment.role_id == "material_taminotchi"
                    || assignment.principal_role == PrincipalRole::MaterialTaminotchi)
        }) {
            Ok("60".to_string())
        } else {
            Ok("30".to_string())
        }
    }

    pub async fn regenerate_werka_code(&self) -> Result<AdminSettings, AdminPortError> {
        let mut state = self.state_for("werka").await?;
        let now = OffsetDateTime::now_utc();
        state = bump_code_regen_state(state, now)?;
        let mut existing = BTreeMap::new();
        let code = random_code(&self.config.read().await.werka_prefix, &mut existing);
        state.custom_code = code.clone();
        if let Some(port) = &self.state_port
            && port.builtin_identity("werka").await?.is_none() {
            let config = self.config.read().await;
            port.put_builtin_identity("werka", &config.werka_phone, &config.werka_name).await?;
        }
        self.put_state("werka", state).await?;
        // Keep only a hash in the in-memory compatibility identity.
        self.config.write().await.werka_code = crate::core::auth::password::hash_password(code.clone())
            .await.map_err(|_| AdminPortError::LookupFailed)?;
        let config = self.config.read().await;
        self.update_auth_runtime(
            &config.werka_phone,
            &config.werka_code,
            &config.werka_name,
            &config.admin_phone,
            &config.admin_name,
        );
        drop(config);
        let mut settings = self.settings().await?;
        settings.werka_code = code;
        Ok(settings)
    }
}
