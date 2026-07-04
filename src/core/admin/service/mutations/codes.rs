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
        let prefix = self.worker_access_code_prefix(&worker.id).await?;
        state.custom_code = random_code(&prefix, &mut existing);
        self.put_state(&worker.id, state).await?;
        self.worker_detail(worker).await
    }

    async fn worker_access_code_prefix(&self, ref_: &str) -> Result<String, AdminPortError> {
        let assignments = self.role_assignments().await?;
        let ref_ = ref_.trim();
        if assignments.iter().any(|assignment| {
            assignment.principal_ref.trim() == ref_
                && (assignment.role_id == "qolipchi"
                    || assignment.principal_role == PrincipalRole::Qolipchi)
        }) {
            Ok("50".to_string())
        } else {
            Ok("40".to_string())
        }
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
