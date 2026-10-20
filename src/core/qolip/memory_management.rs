fn memory_management_owner(warehouse: &str, allowed: Option<&[String]>) -> Result<(), QolipError> {
    if warehouse.trim().is_empty()
        || allowed.is_some_and(|values| {
            !values
                .iter()
                .any(|value| value.eq_ignore_ascii_case(warehouse))
        })
    {
        return Err(QolipError::AccessDenied);
    }
    Ok(())
}

impl MemoryQolipStore {
    async fn memory_managed_sets(
        &self,
        allowed: Option<&[String]>,
    ) -> Result<Vec<super::QolipManagedSet>, QolipError> {
        let specs = self.product_specs.read().await;
        let products = self.products.read().await;
        let links = self.shared_links.read().await;
        let overrides = self.product_overrides.read().await;
        let mut registry = self.managed_registry.write().await;
        let ids = specs
            .values()
            .filter(|s| !s.set_id().starts_with("qolip-local:"))
            .map(|s| s.set_id())
            .chain(registry.keys().cloned())
            .collect::<BTreeSet<_>>();
        let mut result = Vec::new();
        for id in ids {
            let snapshot =
                memory_shared_snapshot(&specs, &products, &links, &overrides, &id, &mut registry)?
                    .ok_or(QolipError::SetChanged)?;
            let mut set = match registry.get(&id) {
                Some(set) => set.clone(),
                None => {
                    let first = snapshot.members.first().ok_or(QolipError::SetChanged)?;
                    super::QolipManagedSet {
                        snapshot: snapshot.clone(),
                        warehouse: first.warehouse.clone(),
                        item_code: first.item_code.clone(),
                        item_name: first.item_name.clone(),
                        item_group: first.item_group.clone(),
                    }
                }
            };
            set.snapshot = snapshot;
            let targets = memory_set_targets(&links, &id, &set.item_code);
            set.snapshot
                .products
                .retain(|product| targets.contains(&product.code.to_lowercase()));
            registry.insert(id, set.clone());
            if !set.snapshot.members.is_empty()
                && memory_management_owner(&set.warehouse, allowed).is_ok()
            {
                result.push(set);
            }
        }
        result.sort_by_key(|set| set.snapshot.display_code.to_lowercase());
        Ok(result)
    }

    async fn memory_edit_set(
        &self,
        input: &super::QolipSetEdit,
        principal: &Principal,
        allowed: Option<&[String]>,
    ) -> Result<super::QolipManagedSet, QolipError> {
        self.memory_managed_sets(allowed).await?;
        let mut specs = self.product_specs.write().await;
        let products = self.products.read().await;
        let links = self.shared_links.read().await;
        let mut overrides = self.product_overrides.write().await;
        let mut registry = self.managed_registry.write().await;
        let current = memory_shared_snapshot(
            &specs,
            &products,
            &links,
            &overrides,
            &input.set_id,
            &mut registry,
        )?
        .ok_or(QolipError::SetChanged)?;
        if current.fingerprint != input.fingerprint {
            return Err(QolipError::SetChanged);
        }
        let mut set = registry
            .get(&input.set_id)
            .cloned()
            .ok_or(QolipError::SetChanged)?;
        memory_management_owner(&set.warehouse, allowed)?;
        let checkouts = self.checkouts.read().await;
        if current.members.iter().any(|spec| {
            checkouts.iter().any(|checkout| {
                checkout.status == "open"
                    && checkout.qolip_code.eq_ignore_ascii_case(&spec.qolip_code)
            })
        }) {
            return Err(QolipError::QolipInUse);
        }
        let removed = input
            .remove
            .iter()
            .map(|c| c.trim().to_lowercase())
            .collect::<BTreeSet<_>>();
        if removed.iter().any(|code| {
            !current
                .members
                .iter()
                .any(|s| s.qolip_code.to_lowercase() == *code)
        }) {
            return Err(QolipError::SetChanged);
        }
        let mut additions = Vec::new();
        for draft in &input.add {
            let spec = draft.spec(
                (&set.item_code, &set.item_name, &set.item_group),
                &set.warehouse,
                &input.set_id,
                principal,
            )?;
            if specs
                .values()
                .any(|s| s.qolip_code.eq_ignore_ascii_case(&spec.qolip_code))
                || registry.values().any(|other| {
                    other
                        .snapshot
                        .display_code
                        .eq_ignore_ascii_case(&spec.qolip_code)
                })
            {
                return Err(QolipError::QolipCodeConflict);
            }
            additions.push(spec);
        }
        specs.retain(|_, spec| !removed.contains(&spec.qolip_code.to_lowercase()));
        overrides.retain(|(_, code), _| !removed.contains(code));
        self.locations
            .write()
            .await
            .retain(|location| !removed.contains(&location.qolip_code.to_lowercase()));
        for spec in additions {
            specs.insert(spec.qolip_code.to_lowercase(), spec);
        }
        set.snapshot = memory_shared_snapshot(
            &specs,
            &products,
            &links,
            &overrides,
            &input.set_id,
            &mut registry,
        )?
        .ok_or(QolipError::SetChanged)?;
        let targets = memory_set_targets(&links, &input.set_id, &set.item_code);
        set.snapshot
            .products
            .retain(|product| targets.contains(&product.code.to_lowercase()));
        registry.insert(input.set_id.clone(), set.clone());
        Ok(set)
    }

    async fn memory_edit_products(
        &self,
        input: &super::QolipProductMoldEdits,
        principal: &Principal,
        allowed: Option<&[String]>,
    ) -> Result<(), QolipError> {
        let mut stored = self.product_specs.write().await;
        let products = self.products.read().await;
        let mut overrides = self.product_overrides.write().await;
        let checkouts = self.checkouts.read().await;
        let mut specs = stored.clone();
        let mut next = overrides.clone();
        for change in &input.changes {
            let item = products
                .iter()
                .find(|p| p.code.eq_ignore_ascii_case(change.item_code.trim()))
                .ok_or(QolipError::MissingItem)?;
            for code in change
                .add
                .iter()
                .map(|s| &s.qolip_code)
                .chain(&change.remove)
            {
                if checkouts
                    .iter()
                    .any(|c| c.status == "open" && c.qolip_code.eq_ignore_ascii_case(code.trim()))
                {
                    return Err(QolipError::QolipInUse);
                }
            }
            for draft in &change.add {
                let code = draft.qolip_code.trim().to_lowercase();
                let spec = if let Some(spec) =
                    specs.values().find(|s| s.qolip_code.to_lowercase() == code)
                {
                    if !draft.existing_only {
                        return Err(QolipError::QolipCodeConflict);
                    }
                    spec.clone()
                } else {
                    if draft.existing_only {
                        return Err(QolipError::QolipCodeNotFound);
                    }
                    let id = format!("qolip-local:{:032x}", rand::random::<u128>());
                    draft.spec(
                        (&item.code, &item.name, &item.item_group),
                        &change.warehouse,
                        &id,
                        principal,
                    )?
                };
                memory_management_owner(&spec.warehouse, allowed)?;
                if spec.item_code.eq_ignore_ascii_case(&item.code)
                    && !spec.item_group.eq_ignore_ascii_case(&item.item_group)
                {
                    return Err(QolipError::ProductTransferConflict);
                }
                specs.insert(code.clone(), spec);
                next.insert((item.code.to_lowercase(), code), true);
            }
            for code in &change.remove {
                let code = code.trim().to_lowercase();
                let spec = specs
                    .values()
                    .find(|s| s.qolip_code.to_lowercase() == code)
                    .ok_or(QolipError::QolipCodeNotFound)?;
                memory_management_owner(&spec.warehouse, allowed)?;
                next.insert((item.code.to_lowercase(), code), false);
            }
        }
        *stored = specs;
        *overrides = next;
        Ok(())
    }
}
