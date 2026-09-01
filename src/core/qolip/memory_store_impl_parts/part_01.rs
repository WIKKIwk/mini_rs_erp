impl MemoryQolipStore {
    async fn assigned_warehouses(&self, _principal: &Principal) -> Result<Vec<String>, QolipError> {
        let mut warehouses = self
            .blocks
            .read()
            .await
            .iter()
            .map(|block| block.warehouse.trim().to_string())
            .filter(|warehouse| !warehouse.is_empty())
            .collect::<Vec<_>>();
        warehouses.sort_by_cached_key(|warehouse| warehouse.to_lowercase());
        warehouses.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        Ok(warehouses)
    }

    async fn assigned_blocks(&self, _principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(self.blocks.read().await.clone())
    }

    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(self.blocks.read().await.clone())
    }

    async fn rename_block(
        &self,
        block: &str,
        new_block: &str,
        warehouse: &str,
    ) -> Result<QolipBlock, QolipError> {
        let block = block.trim();
        let new_block = new_block.trim();
        let warehouse = warehouse.trim();
        let mut blocks = self.blocks.write().await;
        let Some(index) = blocks
            .iter()
            .position(|item| item.name.trim().eq_ignore_ascii_case(block))
        else {
            return Err(QolipError::MissingBlock);
        };
        if blocks.iter().enumerate().any(|(candidate_index, item)| {
            candidate_index != index && item.name.trim().eq_ignore_ascii_case(new_block)
        }) {
            return Err(QolipError::StoreFailed);
        }
        let resolved_warehouse = if warehouse.is_empty() {
            blocks[index].warehouse.clone()
        } else {
            warehouse.to_string()
        };
        blocks[index] = QolipBlock {
            name: new_block.to_string(),
            warehouse: resolved_warehouse.clone(),
        };
        drop(blocks);

        for location in self.locations.write().await.iter_mut() {
            if location.block.trim().eq_ignore_ascii_case(block) {
                location.block = new_block.to_string();
                location.warehouse = resolved_warehouse.clone();
            }
        }
        for cell in self.cell_qrs.write().await.values_mut() {
            if cell.block.trim().eq_ignore_ascii_case(block) {
                cell.block = new_block.to_string();
                cell.warehouse = resolved_warehouse.clone();
            }
        }
        for checkout in self.checkouts.write().await.iter_mut() {
            if checkout.block.trim().eq_ignore_ascii_case(block) {
                checkout.block = new_block.to_string();
                checkout.warehouse = resolved_warehouse.clone();
            }
        }
        Ok(QolipBlock {
            name: new_block.to_string(),
            warehouse: resolved_warehouse,
        })
    }

    async fn products(
        &self,
        query: &str,
        limit: usize,
        with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        let query = query.trim().to_lowercase();
        let in_use_codes = {
            let checkouts = self.checkouts.read().await;
            checkouts
                .iter()
                .filter(|checkout| checkout.status.trim().eq_ignore_ascii_case("open"))
                .map(|checkout| checkout.qolip_code.trim().to_lowercase())
                .collect::<BTreeSet<_>>()
        };
        let products = self.products.read().await.clone();
        let products_by_code = products
            .iter()
            .enumerate()
            .map(|(index, product)| (product.code.trim().to_lowercase(), index))
            .collect::<BTreeMap<_, _>>();
        let mut items = Vec::new();
        let mut seen_qolip_codes = BTreeSet::new();
        let mut item_codes_with_qolip = BTreeSet::new();

        {
            let specs = self.product_specs.read().await;
            for spec in specs.values() {
                let qolip_key = spec.qolip_code.trim().to_lowercase();
                if qolip_key.is_empty() || !seen_qolip_codes.insert(qolip_key.clone()) {
                    continue;
                }
                let item_key = spec.item_code.trim().to_lowercase();
                item_codes_with_qolip.insert(item_key.clone());
                let base = products_by_code
                    .get(&item_key)
                    .map(|index| &products[*index]);
                let item = QolipProduct {
                    code: spec.item_code.clone(),
                    name: base
                        .map(|product| product.name.clone())
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| spec.item_name.clone()),
                    item_group: base
                        .map(|product| product.item_group.clone())
                        .filter(|group| !group.trim().is_empty())
                        .unwrap_or_else(|| spec.item_group.clone()),
                    customer_names: base
                        .map(|product| product.customer_names.clone())
                        .unwrap_or_default(),
                    qolip_code: spec.qolip_code.clone(),
                    first_qolip_code: base
                        .map(|product| product.first_qolip_code.clone())
                        .filter(|code| !code.trim().is_empty())
                        .unwrap_or_else(|| spec.qolip_code.clone()),
                    size: spec.size,
                    color: spec.color.clone(),
                    has_qolip_spec: true,
                    is_in_use: in_use_codes.contains(&qolip_key),
                };
                if memory_product_matches(&item, &query) {
                    items.push(item);
                }
            }
        }

        {
            let locations = self.locations.read().await;
            for location in locations.iter() {
                let qolip_key = location.qolip_code.trim().to_lowercase();
                if qolip_key.is_empty() || !seen_qolip_codes.insert(qolip_key.clone()) {
                    continue;
                }
                let item_key = location.item_code.trim().to_lowercase();
                item_codes_with_qolip.insert(item_key.clone());
                let base = products_by_code
                    .get(&item_key)
                    .map(|index| &products[*index]);
                let item = QolipProduct {
                    code: location.item_code.clone(),
                    name: base
                        .map(|product| product.name.clone())
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| location.item_name.clone()),
                    item_group: base
                        .map(|product| product.item_group.clone())
                        .unwrap_or_default(),
                    customer_names: base
                        .map(|product| product.customer_names.clone())
                        .unwrap_or_default(),
                    qolip_code: location.qolip_code.clone(),
                    first_qolip_code: base
                        .map(|product| product.first_qolip_code.clone())
                        .filter(|code| !code.trim().is_empty())
                        .unwrap_or_else(|| location.qolip_code.clone()),
                    size: location.size,
                    color: String::new(),
                    has_qolip_spec: true,
                    is_in_use: in_use_codes.contains(&qolip_key),
                };
                if memory_product_matches(&item, &query) {
                    items.push(item);
                }
            }
        }

        {
            let checkouts = self.checkouts.read().await;
            for checkout in checkouts
                .iter()
                .filter(|checkout| checkout.status.trim().eq_ignore_ascii_case("open"))
            {
                let qolip_key = checkout.qolip_code.trim().to_lowercase();
                if qolip_key.is_empty() || !seen_qolip_codes.insert(qolip_key.clone()) {
                    continue;
                }
                let item_key = checkout.item_code.trim().to_lowercase();
                item_codes_with_qolip.insert(item_key.clone());
                let base = products_by_code
                    .get(&item_key)
                    .map(|index| &products[*index]);
                let item = QolipProduct {
                    code: checkout.item_code.clone(),
                    name: base
                        .map(|product| product.name.clone())
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| checkout.item_name.clone()),
                    item_group: base
                        .map(|product| product.item_group.clone())
                        .filter(|group| !group.trim().is_empty())
                        .unwrap_or_else(|| checkout.item_group.clone()),
                    customer_names: base
                        .map(|product| product.customer_names.clone())
                        .unwrap_or_default(),
                    qolip_code: checkout.qolip_code.clone(),
                    first_qolip_code: base
                        .map(|product| product.first_qolip_code.clone())
                        .filter(|code| !code.trim().is_empty())
                        .unwrap_or_else(|| checkout.qolip_code.clone()),
                    size: checkout.size,
                    color: String::new(),
                    has_qolip_spec: true,
                    is_in_use: true,
                };
                if memory_product_matches(&item, &query) {
                    items.push(item);
                }
            }
        }

        if !with_qolip_only {
            for product in &products {
                if item_codes_with_qolip.contains(&product.code.trim().to_lowercase()) {
                    continue;
                }
                let mut item = product.clone();
                item.is_in_use = false;
                if memory_product_matches(&item, &query) {
                    items.push(item);
                }
            }
        }
        items.sort_by_cached_key(|item| {
            (
                item.name.to_lowercase(),
                item.code.to_lowercase(),
                item.qolip_code.to_lowercase(),
            )
        });
        items.truncate(limit.max(1));
        Ok(items)
    }

    async fn product_spec(&self, item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        let saved = self
            .product_specs
            .read()
            .await
            .values()
            .find(|spec| spec.item_code.trim().eq_ignore_ascii_case(item_code.trim()))
            .cloned();
        if saved.is_some() {
            return Ok(saved);
        }
        let location = self
            .locations
            .read()
            .await
            .iter()
            .find(|location| {
                location
                    .item_code
                    .trim()
                    .eq_ignore_ascii_case(item_code.trim())
            })
            .cloned();
        if let Some(location) = location {
            let item_group = self.product_item_group(&location.item_code).await;
            return Ok(Some(Self::legacy_spec(&location, item_group.as_deref())));
        }
        let checkout = self
            .checkouts
            .read()
            .await
            .iter()
            .find(|checkout| {
                checkout.status.trim().eq_ignore_ascii_case("open")
                    && checkout
                        .item_code
                        .trim()
                        .eq_ignore_ascii_case(item_code.trim())
            })
            .cloned();
        match checkout {
            Some(checkout) => {
                let item_group = self.product_item_group(&checkout.item_code).await;
                Ok(Some(Self::legacy_checkout_spec(
                    &checkout,
                    item_group.as_deref(),
                )))
            }
            None => Ok(None),
        }
    }

    async fn product_specs(&self, item_code: &str) -> Result<Vec<QolipProductSpec>, QolipError> {
        let item_code = item_code.trim();
        let mut specs = self
            .product_specs
            .read()
            .await
            .values()
            .filter(|spec| spec.item_code.trim().eq_ignore_ascii_case(item_code))
            .cloned()
            .collect::<Vec<_>>();
        let mut seen = specs
            .iter()
            .map(|spec| spec.qolip_code.trim().to_lowercase())
            .collect::<BTreeSet<_>>();
        let item_group = self.product_item_group(item_code).await;
        for location in self
            .locations
            .read()
            .await
            .iter()
            .filter(|location| location.item_code.trim().eq_ignore_ascii_case(item_code))
        {
            if seen.insert(location.qolip_code.trim().to_lowercase()) {
                specs.push(Self::legacy_spec(location, item_group.as_deref()));
            }
        }
        for checkout in self.checkouts.read().await.iter().filter(|checkout| {
            checkout.status.trim().eq_ignore_ascii_case("open")
                && checkout.item_code.trim().eq_ignore_ascii_case(item_code)
        }) {
            if seen.insert(checkout.qolip_code.trim().to_lowercase()) {
                specs.push(Self::legacy_checkout_spec(
                    checkout,
                    item_group.as_deref(),
                ));
            }
        }
        specs.sort_by_key(|spec| spec.qolip_code.trim().to_lowercase());
        Ok(specs)
    }

    async fn product_spec_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipProductSpec>, QolipError> {
        let qolip_code = qolip_code.trim();
        let saved = self
            .product_specs
            .read()
            .await
            .values()
            .find(|spec| spec.qolip_code.trim().eq_ignore_ascii_case(qolip_code))
            .cloned();
        if saved.is_some() {
            return Ok(saved);
        }
        let location = self
            .locations
            .read()
            .await
            .iter()
            .find(|location| location.qolip_code.trim().eq_ignore_ascii_case(qolip_code))
            .cloned();
        if let Some(location) = location {
            let item_group = self.product_item_group(&location.item_code).await;
            return Ok(Some(Self::legacy_spec(&location, item_group.as_deref())));
        }
        let checkout = self
            .checkouts
            .read()
            .await
            .iter()
            .find(|checkout| {
                checkout.status.trim().eq_ignore_ascii_case("open")
                    && checkout.qolip_code.trim().eq_ignore_ascii_case(qolip_code)
            })
            .cloned();
        match checkout {
            Some(checkout) => {
                let item_group = self.product_item_group(&checkout.item_code).await;
                Ok(Some(Self::legacy_checkout_spec(
                    &checkout,
                    item_group.as_deref(),
                )))
            }
            None => Ok(None),
        }
    }

}
