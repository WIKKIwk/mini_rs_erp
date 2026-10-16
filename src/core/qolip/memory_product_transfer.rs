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
        let stored = self.product_specs.read().await;
        let mut links = self.shared_links.write().await;
        let mut saved = Vec::new();
        let mut selected_sets = BTreeSet::new();
        for code in &input.qolip_codes {
            if checkouts.iter().any(|checkout| checkout.status == "open" && checkout.qolip_code.eq_ignore_ascii_case(code)) {
                return Err(QolipError::QolipInUse);
            }
            let spec = stored.get(code).ok_or(QolipError::QolipCodeNotFound)?;
            if spec.warehouse.is_empty() || !allowed.iter().any(|warehouse| warehouse.eq_ignore_ascii_case(&spec.warehouse)) { return Err(QolipError::AccessDenied); }
            if !memory_set_targets(&links, &spec.set_id(), &spec.item_code).contains(&input.from_item_code.to_lowercase()) { return Err(QolipError::ProductTransferConflict); }
            selected_sets.insert(spec.set_id());
            let mut projected = spec.clone();
            projected.item_code = target.code.clone();
            projected.item_name = target.name.clone();
            projected.item_group = target.item_group.clone();
            saved.push(projected);
        }
        for set_id in &selected_sets {
            if stored.values().filter(|spec| spec.set_id() == *set_id).any(|spec| !input.qolip_codes.contains(&spec.qolip_code.to_lowercase())) {
                return Err(QolipError::ProductTransferConflict);
            }
        }
        for set_id in selected_sets {
            links.insert((set_id.clone(), target.code.to_lowercase()));
            links.remove(&(set_id, input.from_item_code.to_lowercase()));
        }
        transfers.insert(key, (input.clone(), saved.clone()));
        Ok(saved)
    }
}
