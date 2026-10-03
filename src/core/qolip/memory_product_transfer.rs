impl MemoryQolipStore {
    async fn transfer_specs(
        &self,
        input: &QolipProductTransfer,
        principal: &Principal,
        allowed: &[String],
    ) -> Result<Vec<QolipProductSpec>, QolipError> {
        let key = format!("{}:{}", principal.ref_, input.request_id);
        let mut transfers = self.transfers.write().await;
        if let Some((previous, specs)) = transfers.get(&key) {
            if previous != input {
                return Err(QolipError::ProductTransferConflict);
            }
            if specs.iter().any(|spec| {
                !allowed
                    .iter()
                    .any(|warehouse| warehouse.eq_ignore_ascii_case(&spec.warehouse))
            }) {
                return Err(QolipError::AccessDenied);
            }
            return Ok(specs.clone());
        }
        let products = self.products.read().await.clone();
        let target = products
            .iter()
            .find(|p| p.code.eq_ignore_ascii_case(&input.to_item_code))
            .ok_or(QolipError::MissingItem)?;
        if target.item_group.trim().is_empty() {
            return Err(QolipError::MissingItemGroup);
        }
        let checkouts = self.checkouts.read().await;
        let mut stored = self.product_specs.write().await;
        let mut locations = self.locations.write().await;
        let mut saved = Vec::new();
        for code in &input.qolip_codes {
            if checkouts
                .iter()
                .any(|c| c.status == "open" && c.qolip_code.eq_ignore_ascii_case(code))
            {
                return Err(QolipError::QolipInUse);
            }
            let mut spec = match stored.get(code) {
                Some(spec) => spec.clone(),
                None => {
                    let location = locations
                        .iter()
                        .find(|l| l.qolip_code.eq_ignore_ascii_case(code))
                        .ok_or(QolipError::QolipCodeNotFound)?;
                    let group = products
                        .iter()
                        .find(|p| p.code.eq_ignore_ascii_case(&location.item_code))
                        .map(|p| p.item_group.as_str());
                    Self::legacy_spec(location, group)
                }
            };
            if spec.warehouse.is_empty()
                || !allowed
                    .iter()
                    .any(|w| w.eq_ignore_ascii_case(&spec.warehouse))
            {
                return Err(QolipError::AccessDenied);
            }
            if !spec.item_code.eq_ignore_ascii_case(&input.from_item_code) {
                return Err(QolipError::ProductTransferConflict);
            }
            spec.item_code = target.code.clone();
            spec.item_name = target.name.clone();
            spec.item_group = target.item_group.clone();
            saved.push(spec);
        }
        // Validate the complete batch first. Preserve the stock and checkout history.
        let mut updated_locations = locations.clone();
        for spec in &saved {
            for location in updated_locations
                .iter_mut()
                .filter(|l| l.qolip_code.eq_ignore_ascii_case(&spec.qolip_code))
            {
                location.item_code = spec.item_code.clone();
                location.item_name = spec.item_name.clone();
                location.id = qolip_location_id(
                    &location.block,
                    &location.item_code,
                    &location.qolip_code,
                    location.size,
                    &location.row_letter,
                    location.column_number,
                );
            }
        }
        let ids = updated_locations
            .iter()
            .map(|l| &l.id)
            .collect::<BTreeSet<_>>();
        if ids.len() != updated_locations.len() {
            return Err(QolipError::QolipCodeConflict);
        }
        *locations = updated_locations;
        for spec in &saved {
            stored.insert(spec.qolip_code.to_lowercase(), spec.clone());
        }
        transfers.insert(key, (input.clone(), saved.clone()));
        Ok(saved)
    }
}
