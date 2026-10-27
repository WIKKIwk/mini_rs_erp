#[async_trait]
impl QolipStorePort for MemoryQolipStore {
    async fn active_rework_codes(&self, codes: &[String]) -> Result<Vec<String>, QolipError> { Ok(self.memory_active_rework_codes(codes).await) }
    async fn reworks(&self, status: &str, allowed: Option<&[String]>) -> Result<Vec<super::QolipRework>, QolipError> { self.memory_reworks(status, allowed).await }
    async fn send_reworks(&self, input: &super::QolipReworkSend, principal: &Principal, allowed: Option<&[String]>) -> Result<Vec<super::QolipRework>, QolipError> { self.memory_send_reworks(input, principal, allowed).await }
    async fn return_reworks(&self, input: &super::QolipReworkReturn, principal: &Principal, allowed: Option<&[String]>) -> Result<Vec<super::QolipRework>, QolipError> { self.memory_return_reworks(input, principal, allowed).await }
    async fn mileage(&self, code: &str) -> Result<super::QolipMileage, QolipError> { self.memory_mileage(code).await }

    async fn next_set_id(&self) -> Result<String, QolipError> {
        loop {
            let id = super::set_ids::next_memory_set_id()?;
            let specs = self.product_specs.read().await;
            let registry = self.managed_registry.read().await;
            if !registry.contains_key(&id)
                && !specs.values().any(|spec| spec.set_id() == id || spec.qolip_code == id)
            {
                return Ok(id);
            }
        }
    }
    async fn managed_sets(&self, allowed: Option<&[String]>) -> Result<Vec<super::QolipManagedSet>, QolipError> {
        self.memory_managed_sets(allowed).await
    }
    async fn rename_set(&self, input: &super::QolipSetRename, _principal: &Principal, allowed: Option<&[String]>) -> Result<super::QolipManagedSet, QolipError> {
        self.memory_rename_set(input, allowed).await
    }
    async fn create_set(&self, input: &super::QolipSetCreate, principal: &Principal, allowed: &[String]) -> Result<super::QolipManagedSet, QolipError> {
        self.memory_create_set(input, principal, allowed).await
    }
    async fn edit_set(&self, input: &super::QolipSetEdit, principal: &Principal, allowed: Option<&[String]>) -> Result<super::QolipManagedSet, QolipError> {
        let state = self.reworks.read().await;
        if state.entries.values().any(|entry| entry.returned_at.is_none() && entry.qolip_set_id == input.set_id) { return Err(QolipError::QolipInRework); }
        self.memory_edit_set(input, principal, allowed).await
    }
    async fn edit_product_molds(&self, input: &super::QolipProductMoldEdits, principal: &Principal, allowed: Option<&[String]>) -> Result<(), QolipError> {
        let state = self.reworks.read().await;
        let codes = input.changes.iter().flat_map(|change| change.add.iter().map(|draft| draft.qolip_code.clone()).chain(change.remove.iter().cloned())).collect::<Vec<_>>();
        state.ensure_available(&codes)?;
        self.memory_edit_products(input, principal, allowed).await
    }
    async fn product_by_code(&self, item_code: &str) -> Result<Option<QolipProduct>, QolipError> {
        let item_code = item_code.trim();
        if item_code.is_empty() {
            return Ok(None);
        }
        if let Some(product) = self
            .products
            .read()
            .await
            .iter()
            .find(|product| product.code.trim().eq_ignore_ascii_case(item_code))
        {
            return Ok(Some(QolipProduct {
                code: product.code.clone(),
                name: product.name.clone(),
                item_group: product.item_group.clone(),
                ..Default::default()
            }));
        }
        // The in-memory catalog can also be populated directly through mold writes.
        Ok(self
            .product_specs(item_code)
            .await?
            .into_iter()
            .next()
            .map(|spec| QolipProduct {
                code: spec.item_code,
                name: spec.item_name,
                item_group: spec.item_group,
                ..Default::default()
            }))
    }
    async fn resolve_shared_set(&self, identifier: &str) -> Result<Option<super::QolipSetSnapshot>, QolipError> { self.memory_resolve_set(identifier).await }
    async fn linked_product_specs(&self, item_code: &str) -> Result<Vec<QolipProductSpec>, QolipError> { Ok(self.memory_linked_specs(item_code).await) }
    async fn reuse_shared_set(&self, set_id: &str, item_code: &str, fingerprint: &str, _principal: &Principal, allowed: &[String]) -> Result<super::QolipSetSnapshot, QolipError> { {
        let state = self.reworks.read().await;
        if state.entries.values().any(|entry| entry.returned_at.is_none() && entry.qolip_set_id == set_id) { return Err(QolipError::QolipInRework); }
        self.memory_reuse_set(set_id, item_code, fingerprint, allowed).await
    } }
    async fn transfer_product_specs(
        &self,
        input: &QolipProductTransfer,
        principal: &Principal,
        allowed_warehouses: &[String],
    ) -> Result<Vec<QolipProductSpec>, QolipError> {
        let state = self.reworks.read().await;
        state.ensure_available(&input.qolip_codes)?;
        self.transfer_specs(input, principal, allowed_warehouses).await
    }
    async fn order_products(&self, item_codes: &[String]) -> Result<Vec<QolipProduct>, QolipError> {
        let products = self.products.read().await.clone();
        let mut result = Vec::new();
        for product in products {
            if !item_codes.iter().any(|code| code.trim().eq_ignore_ascii_case(product.code.trim())) {
                continue;
            }
            let mut specs = self.product_specs(&product.code).await?;
            specs.extend(self.memory_linked_specs(&product.code).await);
            let has_qolip_spec = !product.item_group.trim().is_empty() && specs.iter().any(|spec| {
                (!spec.item_code.trim().eq_ignore_ascii_case(product.code.trim())
                    || spec.item_group.trim().eq_ignore_ascii_case(product.item_group.trim()))
                    && !spec.qolip_code.trim().is_empty()
            });
            result.push(QolipProduct {
                qolip_set_id: String::new(),
                code: product.code, name: product.name, item_group: product.item_group,
                has_qolip_spec, ..Default::default()
            });
        }
        Ok(result)
    }

    async fn assigned_warehouses(&self, _principal: &Principal) -> Result<Vec<String>, QolipError> {
        MemoryQolipStore::assigned_warehouses(self, _principal).await
    }

    async fn assigned_blocks(&self, _principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        MemoryQolipStore::assigned_blocks(self, _principal).await
    }

    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError> {
        MemoryQolipStore::all_blocks(self).await
    }

    async fn rename_block(
        &self,
        block: &str,
        new_block: &str,
        warehouse: &str,
    ) -> Result<QolipBlock, QolipError> {
        let state = self.reworks.read().await;
        let codes = self.locations.read().await.iter().filter(|location| location.block.eq_ignore_ascii_case(block)).map(|location| location.qolip_code.clone()).collect::<Vec<_>>();
        state.ensure_available(&codes)?;
        MemoryQolipStore::rename_block(self, block, new_block, warehouse).await
    }

    async fn products(
        &self,
        query: &str,
        limit: usize,
        with_qolip_only: bool,
        allowed_blocks: Option<&[String]>,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        MemoryQolipStore::products(self, query, limit, with_qolip_only, allowed_blocks).await
    }

    async fn product_spec(&self, item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        MemoryQolipStore::product_spec(self, item_code).await
    }

    async fn product_specs(&self, item_code: &str) -> Result<Vec<QolipProductSpec>, QolipError> {
        MemoryQolipStore::product_specs(self, item_code).await
    }

    async fn product_spec_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipProductSpec>, QolipError> {
        MemoryQolipStore::product_spec_by_qolip_code(self, qolip_code).await
    }

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        let state = self.reworks.read().await;
        state.ensure_available(&[spec.qolip_code.clone()])?;
        MemoryQolipStore::put_product_spec(self, spec).await
    }

    async fn put_product_specs(
        &self,
        batch_specs: Vec<QolipProductSpec>,
    ) -> Result<Vec<QolipProductSpec>, QolipError> {
        let state = self.reworks.read().await;
        state.ensure_available(&batch_specs.iter().map(|spec| spec.qolip_code.clone()).collect::<Vec<_>>())?;
        MemoryQolipStore::put_product_specs(self, batch_specs).await
    }

    async fn rename_product_spec(
        &self,
        previous_qolip_code: &str,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        let state = self.reworks.read().await;
        state.ensure_available(&[previous_qolip_code.to_owned(), spec.qolip_code.clone()])?;
        if !previous_qolip_code.trim().eq_ignore_ascii_case(spec.qolip_code.trim())
            && super::mileage::memory_mileage(&*self.mileage.read().await, previous_qolip_code)?.tracking_enabled { return Err(QolipError::QolipHistoryLocked); }
        MemoryQolipStore::rename_product_spec(self, previous_qolip_code, spec).await
    }

    async fn delete_product_specs(&self, qolip_codes: &[String]) -> Result<usize, QolipError> {
        let state = self.reworks.read().await;
        state.ensure_available(qolip_codes)?;
        let mileage = self.mileage.read().await;
        for code in qolip_codes {
            if super::mileage::memory_mileage(&mileage, code)?.tracking_enabled { return Err(QolipError::QolipHistoryLocked); }
        }
        MemoryQolipStore::delete_product_specs(self, qolip_codes).await
    }

    async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        let state = self.reworks.read().await;
        Ok(MemoryQolipStore::locations(self, block).await?.into_iter()
            .filter(|location| state.ensure_available(&[location.qolip_code.clone()]).is_ok()).collect())
    }

    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError> {
        let state = self.reworks.read().await;
        state.ensure_available(&[location.qolip_code.clone()])?;
        MemoryQolipStore::put_location(self, location).await
    }

    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError> {
        MemoryQolipStore::get_or_create_cell_qr(self, cell).await
    }

    async fn location_by_id(&self, location_id: &str) -> Result<Option<QolipLocation>, QolipError> {
        let state = self.reworks.read().await;
        Ok(MemoryQolipStore::location_by_id(self, location_id).await?
            .filter(|location| state.ensure_available(&[location.qolip_code.clone()]).is_ok()))
    }

    async fn location_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        let state = self.reworks.read().await;
        if state.ensure_available(&[qolip_code.to_owned()]).is_err() { return Ok(None); }
        MemoryQolipStore::location_by_qolip_code(self, qolip_code).await
    }

    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError> {
        let state = self.reworks.read().await;
        state.ensure_available(&[checkout.qolip_code.clone()])?;
        MemoryQolipStore::issue_checkout(self, checkout).await
    }

    async fn open_checkout_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipCheckout>, QolipError> {
        MemoryQolipStore::open_checkout_by_qolip_code(self, qolip_code).await
    }

    async fn checkouts(
        &self,
        block: Option<&str>,
        allowed_blocks: Option<&[String]>,
        status: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        MemoryQolipStore::checkouts(self, block, allowed_blocks, status, limit).await
    }

    async fn checkout_by_id(&self, checkout_id: &str) -> Result<Option<QolipCheckout>, QolipError> {
        MemoryQolipStore::checkout_by_id(self, checkout_id).await
    }

    async fn return_checkout(
        &self,
        checkout_id: &str,
        row_letter: &str,
        column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError> {
        let state = self.reworks.read().await;
        if let Some(checkout) = MemoryQolipStore::checkout_by_id(self, checkout_id).await? {
            state.ensure_available(&[checkout.qolip_code])?;
        }
        MemoryQolipStore::return_checkout(self, checkout_id, row_letter, column_number).await
    }

    async fn move_location(
        &self,
        location_id: &str,
        block: &str,
        warehouse: &str,
        row_letter: &str,
        column_number: i32,
        quantity: i32,
    ) -> Result<QolipLocation, QolipError> {
        let state = self.reworks.read().await;
        if let Some(location) = MemoryQolipStore::location_by_id(self, location_id).await? {
            state.ensure_available(&[location.qolip_code])?;
        }
        MemoryQolipStore::move_location(self, location_id, block, warehouse, row_letter, column_number, quantity).await
    }

    async fn move_locations(
        &self,
        moves: &[QolipLocationMove],
    ) -> Result<Vec<QolipLocation>, QolipError> {
        let state = self.reworks.read().await;
        let mut codes = Vec::new();
        for input in moves {
            if let Some(location) = MemoryQolipStore::location_by_id(self, &input.location_id).await? { codes.push(location.qolip_code); }
        }
        state.ensure_available(&codes)?;
        MemoryQolipStore::move_locations(self, moves).await
    }

    async fn cell_qr_by_payload(
        &self,
        qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        MemoryQolipStore::cell_qr_by_payload(self, qr_payload).await
    }
}
