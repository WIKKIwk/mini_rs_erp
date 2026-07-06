use super::super::helpers::*;
use super::super::*;

impl AdminService {
    pub async fn create_material_taminotchi(
        &self,
        name: &str,
        phone: &str,
        assigned_item_groups: Vec<String>,
    ) -> Result<AdminCustomerDetail, AdminPortError> {
        let assigned_item_groups = dedupe_strings(assigned_item_groups);
        if assigned_item_groups.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "material taminotchi needs at least one item group".to_string(),
            ));
        }
        let normalized = normalize_admin_phone(phone)?;
        for query in phone_search_terms(phone, &normalized) {
            let existing = self
                .read_port()?
                .material_taminotchilar_page(&query, 50, 0)
                .await?;
            if existing
                .iter()
                .any(|entry| phone_matches(&entry.phone, &normalized))
            {
                return Err(AdminPortError::InvalidInput(
                    "phone already exists".to_string(),
                ));
            }
        }

        let entry = self
            .write_port()?
            .create_material_taminotchi(name.trim(), &normalized)
            .await?;
        let mut existing_codes = self.existing_state_codes().await?;
        let mut state = self.state_for(&entry.ref_).await?;
        state.custom_code = random_code("70", &mut existing_codes);
        self.put_state(&entry.ref_, state.clone()).await?;
        self.write_port()?
            .update_material_taminotchi_code(&entry.ref_, &state.custom_code)
            .await?;
        self.upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: entry.ref_.clone(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups,
        })
        .await?;

        self.material_taminotchi_detail(&entry.ref_).await
    }

    pub async fn material_taminotchi_detail(
        &self,
        ref_: &str,
    ) -> Result<AdminCustomerDetail, AdminPortError> {
        let read = self.read_port()?;
        let entry = read.material_taminotchi_by_ref(ref_.trim()).await?;
        let state = self.state_for(&entry.ref_).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        let avatar_url = self
            .profile_avatar_url("material_taminotchi", &entry.ref_)
            .await;
        let now = OffsetDateTime::now_utc();
        Ok(AdminCustomerDetail {
            ref_: entry.ref_,
            name: entry.name,
            phone: entry.phone,
            avatar_url,
            code: state.custom_code.trim().to_string(),
            code_locked: state.code_locked(now),
            code_retry_after_sec: state.retry_after_seconds(now),
            assigned_items: Vec::new(),
        })
    }

    pub async fn update_material_taminotchi_phone(
        &self,
        ref_: &str,
        phone: &str,
    ) -> Result<AdminCustomerDetail, AdminPortError> {
        let normalized = normalize_admin_phone(phone)?;
        let entry = self
            .read_port()?
            .material_taminotchi_by_ref(ref_.trim())
            .await?;
        let state = self.state_for(&entry.ref_).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        self.write_port()?
            .update_material_taminotchi_phone(&entry.ref_, &normalized)
            .await?;
        let mut detail = self.material_taminotchi_detail(&entry.ref_).await?;
        detail.phone = normalized;
        Ok(detail)
    }

    pub async fn regenerate_material_taminotchi_code(
        &self,
        ref_: &str,
    ) -> Result<AdminCustomerDetail, AdminPortError> {
        let entry = self
            .read_port()?
            .material_taminotchi_by_ref(ref_.trim())
            .await?;
        let mut state = self.state_for(&entry.ref_).await?;
        if state.removed {
            return Err(AdminPortError::NotFound);
        }
        let mut existing = self.existing_state_codes().await?;
        let now = OffsetDateTime::now_utc();
        state = bump_code_regen_state(state, now)?;
        state.custom_code = random_code("70", &mut existing);
        self.put_state(&entry.ref_, state.clone()).await?;
        self.write_port()?
            .update_material_taminotchi_code(&entry.ref_, &state.custom_code)
            .await?;
        self.material_taminotchi_detail(&entry.ref_).await
    }
}
