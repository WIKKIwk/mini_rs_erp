impl MemoryQolipStore {
    async fn memory_reworks(
        &self,
        status: &str,
        allowed: Option<&[String]>,
    ) -> Result<Vec<super::QolipRework>, QolipError> {
        let state = self.reworks.read().await;
        let mut entries = state
            .entries
            .values()
            .filter(|entry| {
                super::reworks::ensure_rework_warehouse(&entry.warehouse, allowed).is_ok()
                    && (status == "all" || (status == "open") == entry.returned_at.is_none())
            })
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| b.sent_at.cmp(&a.sent_at).then_with(|| a.id.cmp(&b.id)));
        Ok(entries)
    }

    async fn memory_active_rework_codes(&self, codes: &[String]) -> Vec<String> {
        self.reworks
            .read()
            .await
            .entries
            .values()
            .filter(|entry| {
                entry.returned_at.is_none()
                    && codes
                        .iter()
                        .any(|code| entry.qolip_code.eq_ignore_ascii_case(code.trim()))
            })
            .map(|entry| entry.qolip_code.clone())
            .collect()
    }

    async fn memory_send_reworks(
        &self,
        input: &super::QolipReworkSend,
        principal: &Principal,
        allowed: Option<&[String]>,
    ) -> Result<Vec<super::QolipRework>, QolipError> {
        let key = (
            format!("{}:{}", super::role_code(&principal.role), principal.ref_),
            input.request_id.clone(),
        );
        let request = serde_json::to_value(input).map_err(|_| QolipError::StoreFailed)?;
        let mut state = self.reworks.write().await;
        if let Some((action, previous, response)) = state.requests.get(&key) {
            if action != "send" || previous != &request {
                return Err(QolipError::ReworkRequestConflict);
            }
            for entry in response {
                super::reworks::ensure_rework_warehouse(&entry.warehouse, allowed)?;
            }
            return Ok(response.clone());
        }
        state.ensure_available(&input.qolip_codes)?;
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| QolipError::StoreFailed)?;
        let mut response = Vec::with_capacity(input.qolip_codes.len());
        for code in &input.qolip_codes {
            let spec = MemoryQolipStore::product_spec_by_qolip_code(self, code)
                .await?
                .ok_or(QolipError::QolipCodeNotFound)?;
            super::reworks::ensure_rework_warehouse(&spec.warehouse, allowed)?;
            if self.checkouts.read().await.iter().any(|entry| {
                entry.status.eq_ignore_ascii_case("open")
                    && entry.qolip_code.eq_ignore_ascii_case(code)
            }) {
                return Err(QolipError::QolipInUse);
            }
            response.push(super::QolipRework {
                id: format!("qolip-rework:{:032x}", rand::random::<u128>()),
                qolip_code: spec.qolip_code.clone(),
                qolip_set_id: spec.set_id(),
                warehouse: spec.warehouse,
                item_code: spec.item_code,
                item_name: spec.item_name,
                color: spec.color,
                size: spec.size,
                sent_at: now.clone(),
                returned_at: None,
            });
        }
        for entry in &response {
            state.entries.insert(entry.id.clone(), entry.clone());
        }
        state
            .requests
            .insert(key, ("send".into(), request, response.clone()));
        Ok(response)
    }

    async fn memory_return_reworks(
        &self,
        input: &super::QolipReworkReturn,
        principal: &Principal,
        allowed: Option<&[String]>,
    ) -> Result<Vec<super::QolipRework>, QolipError> {
        let key = (
            format!("{}:{}", super::role_code(&principal.role), principal.ref_),
            input.request_id.clone(),
        );
        let request = serde_json::to_value(input).map_err(|_| QolipError::StoreFailed)?;
        let mut state = self.reworks.write().await;
        if let Some((action, previous, response)) = state.requests.get(&key) {
            if action != "return" || previous != &request {
                return Err(QolipError::ReworkRequestConflict);
            }
            for entry in response {
                super::reworks::ensure_rework_warehouse(&entry.warehouse, allowed)?;
            }
            return Ok(response.clone());
        }
        let returned_at = time::OffsetDateTime::now_utc();
        let now = returned_at
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| QolipError::StoreFailed)?;
        let mut response = Vec::with_capacity(input.rework_ids.len());
        for id in &input.rework_ids {
            let mut entry = state
                .entries
                .get(id)
                .cloned()
                .ok_or(QolipError::ReworkConflict)?;
            super::reworks::ensure_rework_warehouse(&entry.warehouse, allowed)?;
            if entry.returned_at.is_some() {
                return Err(QolipError::ReworkConflict);
            }
            entry.returned_at = Some(now.clone());
            response.push(entry);
        }
        let mut mileage = self.mileage.write().await;
        let mut staged = mileage.clone();
        for entry in &response {
            super::mileage::begin_memory_cycle(
                &mut staged,
                &entry.id,
                &entry.qolip_code,
                returned_at,
            )?;
        }
        for entry in &response {
            state.entries.insert(entry.id.clone(), entry.clone());
        }
        *mileage = staged;
        state
            .requests
            .insert(key, ("return".into(), request, response.clone()));
        Ok(response)
    }
}
