impl QolipService {
    pub async fn reworks(
        &self,
        status: &str,
        principal: &Principal,
        is_admin: bool,
    ) -> Result<Vec<super::QolipRework>, QolipError> {
        super::reworks::validate_rework_status(status)?;
        let allowed = self.warehouses_for_principal(principal, false).await?;
        self.store
            .reworks(status, if is_admin { None } else { Some(&allowed) })
            .await
    }

    pub async fn send_reworks(
        &self,
        mut input: super::QolipReworkSend,
        principal: &Principal,
        is_admin: bool,
    ) -> Result<Vec<super::QolipRework>, QolipError> {
        super::reworks::normalize_request(&mut input.request_id, &mut input.qolip_codes, true)?;
        let allowed = self.warehouses_for_principal(principal, false).await?;
        self.store
            .send_reworks(
                &input,
                principal,
                if is_admin { None } else { Some(&allowed) },
            )
            .await
    }

    pub async fn return_reworks(
        &self,
        mut input: super::QolipReworkReturn,
        principal: &Principal,
        is_admin: bool,
    ) -> Result<Vec<super::QolipRework>, QolipError> {
        super::reworks::normalize_request(&mut input.request_id, &mut input.rework_ids, false)?;
        let allowed = self.warehouses_for_principal(principal, false).await?;
        self.store
            .return_reworks(
                &input,
                principal,
                if is_admin { None } else { Some(&allowed) },
            )
            .await
    }
}
