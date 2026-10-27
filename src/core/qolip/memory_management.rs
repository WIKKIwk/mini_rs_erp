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
    async fn memory_create_set(
        &self,
        input: &super::QolipSetCreate,
        principal: &Principal,
        allowed: &[String],
    ) -> Result<super::QolipManagedSet, QolipError> {
        memory_management_owner(input.warehouse.trim(), Some(allowed))?;
        let mut specs = self.product_specs.write().await;
        let mut registry = self.managed_registry.write().await;
        let locations = self.locations.read().await;
        let checkouts = self.checkouts.read().await;
        let codes = input.members.iter().map(|draft| draft.qolip_code.trim().to_lowercase()).collect::<BTreeSet<_>>();
        if specs.values().any(|s| codes.contains(&s.qolip_code.to_lowercase()) || codes.contains(&s.set_id()))
            || registry.keys().any(|id| codes.contains(id))
            || locations.iter().any(|m| codes.contains(&m.qolip_code.to_lowercase()))
            || checkouts.iter().any(|m| codes.contains(&m.qolip_code.to_lowercase()))
        {
            return Err(QolipError::QolipCodeConflict);
        }
        let id = loop {
            let candidate = super::set_ids::next_memory_set_id()?;
            if !registry.contains_key(&candidate)
                && !codes.contains(&candidate)
                && !specs.values().any(|s| s.set_id() == candidate || s.qolip_code == candidate)
                && !locations.iter().any(|s| s.qolip_code == candidate)
                && !checkouts.iter().any(|s| s.qolip_code == candidate)
            {
                break candidate;
            }
        };
        let members = input.members.iter()
            .map(|draft| draft.standalone_spec(&input.warehouse, &id, principal))
            .collect::<Result<Vec<_>, _>>()?;
        let set = super::QolipManagedSet {
            name: String::new(),
            snapshot: super::QolipSetSnapshot::new(id.clone(), members.clone(), Vec::new()),
            warehouse: input.warehouse.trim().to_string(),
            item_code: String::new(),
            item_name: String::new(),
            item_group: String::new(),
        };
        for member in members {
            specs.insert(member.qolip_code.to_lowercase(), member);
        }
        registry.insert(id, set.clone());
        Ok(set)
    }

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
            .filter(|s| !s.is_extension)
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
                        name: String::new(),
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
        result.sort_by_key(|set| set.snapshot.set_id.clone());
        Ok(result)
    }

    async fn memory_rename_set(
        &self,
        input: &super::QolipSetRename,
        allowed: Option<&[String]>,
    ) -> Result<super::QolipManagedSet, QolipError> {
        self.memory_managed_sets(None).await?;
        let mut registry = self.managed_registry.write().await;
        let current = registry.get(&input.set_id).ok_or(QolipError::SetChanged)?;
        memory_management_owner(&current.warehouse, allowed)?;
        let key = input.name.to_lowercase();
        if registry.iter().any(|(id, set)| *id != input.set_id && set.name.to_lowercase() == key) {
            return Err(QolipError::SetNameConflict);
        }
        let set = registry.get_mut(&input.set_id).ok_or(QolipError::SetChanged)?;
        set.name = input.name.clone();
        Ok(set.clone())
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
            let spec = if set.item_code.is_empty() {
                draft.standalone_spec(&set.warehouse, &input.set_id, principal)?
            } else {
                draft.spec(
                    (&set.item_code, &set.item_name, &set.item_group),
                    &set.warehouse,
                    &input.set_id,
                    principal,
                )?
            };
            if specs
                .values()
                .any(|s| s.qolip_code.eq_ignore_ascii_case(&spec.qolip_code))
                || registry.values().any(|other| {
                    other
                        .snapshot
                        .set_id
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
        let registry = self.managed_registry.read().await;
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
                    let id = loop {
                        let candidate = super::set_ids::next_memory_set_id()?;
                        if !registry.contains_key(&candidate)
                            && !specs.values().any(|spec| {
                                spec.set_id() == candidate || spec.qolip_code == candidate
                            })
                        {
                            break candidate;
                        }
                    };
                    let mut local = draft.spec(
                        (&item.code, &item.name, &item.item_group),
                        &change.warehouse,
                        &id,
                        principal,
                    )?;
                    local.is_extension = true;
                    local
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
