impl MemoryQolipStore {

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        let spec_key = spec.qolip_code.trim().to_lowercase();
        let mut specs = self.product_specs.write().await;
        if specs.contains_key(&spec_key) {
            return Err(QolipError::QolipCodeConflict);
        }
        specs.insert(spec_key, spec.clone());

        let mut products = self.products.write().await;
        if let Some(product) = products.iter_mut().find(|product| {
            product
                .code
                .trim()
                .eq_ignore_ascii_case(spec.item_code.trim())
        }) {
            if product.first_qolip_code.trim().is_empty() {
                product.first_qolip_code = spec.qolip_code.clone();
            }
            product.qolip_code = spec.qolip_code.clone();
            product.size = spec.size;
            product.color = spec.color.clone();
            product.has_qolip_spec = true;
        } else {
            products.push(QolipProduct {
                code: spec.item_code.clone(),
                name: spec.item_name.clone(),
                item_group: spec.item_group.clone(),
                customer_names: Vec::new(),
                qolip_code: spec.qolip_code.clone(),
                first_qolip_code: spec.qolip_code.clone(),
                size: spec.size,
                color: spec.color.clone(),
                has_qolip_spec: true,
                is_in_use: false,
            });
        }
        Ok(spec)
    }

    async fn put_product_specs(
        &self,
        batch_specs: Vec<QolipProductSpec>,
    ) -> Result<Vec<QolipProductSpec>, QolipError> {
        if batch_specs.is_empty() {
            return Err(QolipError::MissingQolipCode);
        }
        let mut stored_specs = self.product_specs.write().await;
        let mut seen = BTreeSet::new();
        for spec in &batch_specs {
            let key = spec.qolip_code.trim().to_lowercase();
            if !seen.insert(key.clone()) || stored_specs.contains_key(&key) {
                return Err(QolipError::QolipCodeConflict);
            }
        }

        let mut products = self.products.write().await;
        for spec in &batch_specs {
            if let Some(product) = products.iter_mut().find(|product| {
                product
                    .code
                    .trim()
                    .eq_ignore_ascii_case(spec.item_code.trim())
            }) {
                if product.first_qolip_code.trim().is_empty() {
                    product.first_qolip_code = spec.qolip_code.clone();
                }
                product.qolip_code = spec.qolip_code.clone();
                product.size = spec.size;
                product.color = spec.color.clone();
                product.has_qolip_spec = true;
            } else {
                products.push(QolipProduct {
                    code: spec.item_code.clone(),
                    name: spec.item_name.clone(),
                    item_group: spec.item_group.clone(),
                    customer_names: Vec::new(),
                    qolip_code: spec.qolip_code.clone(),
                    first_qolip_code: spec.qolip_code.clone(),
                    size: spec.size,
                    color: spec.color.clone(),
                    has_qolip_spec: true,
                    is_in_use: false,
                });
            }
        }
        for spec in &batch_specs {
            stored_specs.insert(spec.qolip_code.trim().to_lowercase(), spec.clone());
        }
        Ok(batch_specs)
    }

    async fn rename_product_spec(
        &self,
        previous_qolip_code: &str,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        let previous_key = previous_qolip_code.trim().to_lowercase();
        let next_key = spec.qolip_code.trim().to_lowercase();
        if previous_key.is_empty() || next_key.is_empty() {
            return Err(QolipError::MissingQolipCode);
        }
        if self.checkouts.read().await.iter().any(|checkout| {
            checkout.status.trim().eq_ignore_ascii_case("open")
                && checkout
                    .qolip_code
                    .trim()
                    .eq_ignore_ascii_case(previous_qolip_code)
        }) {
            return Err(QolipError::QolipInUse);
        }
        if previous_key != next_key
            && self.locations.read().await.iter().any(|location| {
                location
                    .qolip_code
                    .trim()
                    .eq_ignore_ascii_case(&spec.qolip_code)
            })
        {
            return Err(QolipError::QolipCodeConflict);
        }
        let mut specs = self.product_specs.write().await;
        if previous_key != next_key && specs.contains_key(&next_key) {
            return Err(QolipError::QolipCodeConflict);
        }
        if specs.remove(&previous_key).is_none() {
            return Err(QolipError::QolipCodeNotFound);
        }
        specs.insert(next_key, spec.clone());
        drop(specs);
        let mut products = self.products.write().await;
        if let Some(product) = products.iter_mut().find(|product| {
            product
                .qolip_code
                .trim()
                .eq_ignore_ascii_case(previous_qolip_code)
        }) {
            product.qolip_code = spec.qolip_code.clone();
            product.size = spec.size;
            product.color = spec.color.clone();
        }
        drop(products);
        let mut locations = self.locations.write().await;
        for location in locations.iter_mut().filter(|location| {
            location
                .qolip_code
                .trim()
                .eq_ignore_ascii_case(previous_qolip_code)
        }) {
            location.item_code = spec.item_code.clone();
            location.item_name = spec.item_name.clone();
            location.qolip_code = spec.qolip_code.clone();
            location.size = spec.size;
            location.id = qolip_location_id(
                &location.block,
                &location.item_code,
                &location.qolip_code,
                location.size,
                &location.row_letter,
                location.column_number,
            );
        }
        Ok(spec)
    }

    async fn delete_product_specs(&self, qolip_codes: &[String]) -> Result<usize, QolipError> {
        let normalized = qolip_codes
            .iter()
            .map(|code| code.trim().to_lowercase())
            .filter(|code| !code.is_empty())
            .collect::<BTreeSet<_>>();
        if normalized.is_empty() {
            return Err(QolipError::MissingQolipCode);
        }
        if self.checkouts.read().await.iter().any(|checkout| {
            checkout.status.trim().eq_ignore_ascii_case("open")
                && normalized.contains(&checkout.qolip_code.trim().to_lowercase())
        }) {
            return Err(QolipError::QolipInUse);
        }
        let spec_codes = self
            .product_specs
            .read()
            .await
            .values()
            .map(|spec| spec.qolip_code.trim().to_lowercase())
            .collect::<Vec<_>>();
        let location_codes = self
            .locations
            .read()
            .await
            .iter()
            .map(|location| location.qolip_code.trim().to_lowercase())
            .collect::<Vec<_>>();
        let existing_codes = spec_codes
            .into_iter()
            .chain(location_codes)
            .filter(|code| normalized.contains(code))
            .collect::<BTreeSet<_>>();
        let mut specs = self.product_specs.write().await;
        specs.retain(|code, _| !normalized.contains(&code.trim().to_lowercase()));
        drop(specs);
        self.locations
            .write()
            .await
            .retain(|location| !normalized.contains(&location.qolip_code.trim().to_lowercase()));
        Ok(existing_codes.len())
    }

    async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        let block = block.trim().to_lowercase();
        Ok(self
            .locations
            .read()
            .await
            .iter()
            .filter(|location| location.block.to_lowercase() == block)
            .cloned()
            .collect())
    }

    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError> {
        let mut locations = self.locations.write().await;
        locations.retain(|item| {
            !item
                .qolip_code
                .trim()
                .eq_ignore_ascii_case(location.qolip_code.trim())
        });
        locations.push(location.clone());
        locations.sort_by(|left, right| {
            left.row_letter
                .cmp(&right.row_letter)
                .then_with(|| left.column_number.cmp(&right.column_number))
                .then_with(|| left.item_name.cmp(&right.item_name))
        });
        Ok(location)
    }

    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError> {
        let mut cell_qrs = self.cell_qrs.write().await;
        if let Some(existing) = cell_qrs.get(&cell.id) {
            return Ok(existing.clone());
        }
        if let Some(existing) = cell_qrs.values().find(|existing| {
            existing
                .warehouse
                .trim()
                .eq_ignore_ascii_case(cell.warehouse.trim())
                && existing
                    .block
                    .trim()
                    .eq_ignore_ascii_case(cell.block.trim())
                && existing
                    .row_letter
                    .trim()
                    .eq_ignore_ascii_case(cell.row_letter.trim())
                && existing.column_number == cell.column_number
        }) {
            return Ok(existing.clone());
        }
        cell_qrs.insert(cell.id.clone(), cell.clone());
        Ok(cell)
    }

    async fn location_by_id(&self, location_id: &str) -> Result<Option<QolipLocation>, QolipError> {
        let location_id = location_id.trim();
        Ok(self
            .locations
            .read()
            .await
            .iter()
            .find(|location| location.id == location_id)
            .cloned())
    }

    async fn location_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        let qolip_code = qolip_code.trim();
        Ok(self
            .locations
            .read()
            .await
            .iter()
            .find(|location| location.qolip_code.trim().eq_ignore_ascii_case(qolip_code))
            .cloned())
    }

    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError> {
        let mut locations = self.locations.write().await;
        let Some(index) = locations
            .iter()
            .position(|location| location.id == checkout.location_id)
        else {
            return Err(QolipError::LocationNotFound);
        };
        let expected = location_from_checkout(&checkout);
        if !location_identity_matches(&locations[index], &expected) {
            return Err(QolipError::LocationIdentityMismatch);
        }
        if checkout.quantity > locations[index].quantity {
            return Err(QolipError::InsufficientStock);
        }
        let remaining = locations[index].quantity - checkout.quantity;
        if remaining > 0 {
            locations[index].quantity = remaining;
        } else {
            locations.remove(index);
        }
        drop(locations);

        let mut saved = checkout.clone();
        if saved.issued_at.is_empty() {
            saved.issued_at = "1970-01-01T00:00:00Z".to_string();
        }
        self.checkouts.write().await.push(saved.clone());
        Ok(saved)
    }

    async fn open_checkout_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipCheckout>, QolipError> {
        let qolip_code = qolip_code.trim();
        Ok(self
            .checkouts
            .read()
            .await
            .iter()
            .find(|checkout| {
                checkout.status.trim().eq_ignore_ascii_case("open")
                    && checkout.qolip_code.trim().eq_ignore_ascii_case(qolip_code)
            })
            .cloned())
    }

    async fn checkouts(
        &self,
        block: Option<&str>,
        allowed_blocks: Option<&[String]>,
        status: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        let status = status.trim().to_lowercase();
        let block = block.map(str::trim).filter(|value| !value.is_empty());
        let mut items = self
            .checkouts
            .read()
            .await
            .iter()
            .filter(|checkout| checkout.status.to_lowercase() == status)
            .filter(|checkout| {
                if let Some(block) = block {
                    checkout.block.eq_ignore_ascii_case(block)
                } else if let Some(allowed_blocks) = allowed_blocks {
                    allowed_blocks
                        .iter()
                        .any(|block| checkout.block.eq_ignore_ascii_case(block))
                } else {
                    true
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| right.issued_at.cmp(&left.issued_at));
        Ok(items.into_iter().take(limit.max(1)).collect())
    }

    async fn checkout_by_id(&self, checkout_id: &str) -> Result<Option<QolipCheckout>, QolipError> {
        let checkout_id = checkout_id.trim();
        Ok(self
            .checkouts
            .read()
            .await
            .iter()
            .find(|checkout| checkout.id == checkout_id)
            .cloned())
    }
}
