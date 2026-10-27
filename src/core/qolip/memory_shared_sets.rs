fn memory_set_targets(
    links: &BTreeSet<(String, String)>,
    set_id: &str,
    native: &str,
) -> Vec<String> {
    let targets = links
        .iter()
        .filter(|(id, _)| id == set_id)
        .map(|(_, code)| code.clone())
        .collect::<Vec<_>>();
    if targets.is_empty() && !native.trim().is_empty() {
        vec![native.trim().to_lowercase()]
    } else {
        targets
    }
}

fn memory_effective_binding(spec: &QolipProductSpec, item: &str,
    links: &BTreeSet<(String, String)>, overrides: &BTreeMap<(String, String), bool>) -> Option<bool> {
    let item = item.trim().to_lowercase();
    let extra = overrides.get(&(item.clone(), spec.qolip_code.trim().to_lowercase()));
    if extra == Some(&false) { return None; }
    if !spec.is_extension && memory_set_targets(links, &spec.set_id(), &spec.item_code).contains(&item) { return Some(false); }
    (extra == Some(&true)).then_some(true)
}

fn memory_shared_snapshot(
    specs: &BTreeMap<String, QolipProductSpec>,
    products: &[QolipProduct],
    links: &BTreeSet<(String, String)>,
    overrides: &BTreeMap<(String, String), bool>,
    identifier: &str,
    registry: &mut BTreeMap<String, super::QolipManagedSet>,
) -> Result<Option<super::QolipSetSnapshot>, QolipError> {
    let mut groups = BTreeMap::<String, Vec<QolipProductSpec>>::new();
    for id in registry.keys() { groups.insert(id.clone(), Vec::new()); }
    for spec in specs.values().filter(|spec| !spec.is_extension) {
        groups.entry(spec.set_id()).or_default().push(spec.clone());
    }
    for (id, members) in &groups {
        if registry.contains_key(id) { continue; }
        let Some(first) = members.first() else { continue; };
        registry.insert(id.clone(), super::QolipManagedSet {
            name: String::new(),
            snapshot: super::QolipSetSnapshot::new(id.clone(), members.clone(), Vec::new()),
            warehouse: first.warehouse.clone(),
            item_code: first.item_code.clone(),
            item_name: first.item_name.clone(),
            item_group: first.item_group.clone(),
        });
    }
    let matches = |id: &str, members: &[QolipProductSpec], identifiers: &[String]| {
        identifiers.iter().any(|value| {
            id == value
                || members
                    .iter()
                    .any(|spec| spec.qolip_code.eq_ignore_ascii_case(value))
        })
    };
    let mut selected = groups
        .iter()
        .filter(|(id, members)| matches(id, members, &[identifier.to_string()]))
        .collect::<Vec<_>>();
    if selected.is_empty() {
        let candidates = super::candidate_codes(identifier);
        selected = groups
            .iter()
            .filter(|(id, members)| matches(id, members, &candidates))
            .collect();
    }
    if selected.len() > 1 {
        return Err(QolipError::SetAmbiguous);
    }
    let Some((id, members)) = selected.first() else {
        return Ok(None);
    };
    let native = members.first().map(|spec| spec.item_code.as_str())
        .or_else(|| registry.get(*id).map(|set| set.item_code.as_str())).unwrap_or("");
    let mut targets = memory_set_targets(links, id, native);
    targets.extend(overrides.iter().filter(|((_, code), include)| **include && members.iter().any(|spec| spec.qolip_code.eq_ignore_ascii_case(code)))
        .map(|((item, _), _)| item.clone()));
    let products = products
        .iter()
        .filter(|product| targets.contains(&product.code.trim().to_lowercase()))
        .map(|product| super::QolipSetProduct {
            code: product.code.clone(),
            name: product.name.clone(),
        })
        .collect();
    Ok(Some(super::QolipSetSnapshot::new(
        (*id).clone(),
        (*members).clone(),
        products,
    )))
}

impl MemoryQolipStore {
    async fn memory_linked_specs(&self, item_code: &str) -> Vec<QolipProductSpec> {
        let links = self.shared_links.read().await.clone();
        let overrides = self.product_overrides.read().await.clone();
        let products = self.products.read().await.clone();
        let group = products
            .iter()
            .find(|p| p.code.eq_ignore_ascii_case(item_code))
            .map(|p| p.item_group.as_str());
        self.product_specs
            .read()
            .await
            .values()
            .filter_map(|spec| {
                let is_extension = memory_effective_binding(spec, item_code, &links, &overrides)?;
                if spec.item_code.eq_ignore_ascii_case(item_code)
                    && !group.is_some_and(|group| group.eq_ignore_ascii_case(&spec.item_group)) { return None; }
                let mut effective = spec.clone();
                effective.is_extension = is_extension;
                Some(effective)
            })
            .collect()
    }

    async fn memory_resolve_set(
        &self,
        identifier: &str,
    ) -> Result<Option<super::QolipSetSnapshot>, QolipError> {
        let specs = self.product_specs.read().await.clone();
        let products = self.products.read().await.clone();
        let links = self.shared_links.read().await.clone();
        let overrides = self.product_overrides.read().await.clone();
        let mut registry = self.managed_registry.write().await;
        Ok(memory_shared_snapshot(&specs, &products, &links, &overrides, identifier, &mut registry)?
            .filter(|set| !set.members.is_empty()))
    }

    async fn memory_reuse_set(
        &self,
        set_id: &str,
        item_code: &str,
        fingerprint: &str,
        allowed: &[String],
    ) -> Result<super::QolipSetSnapshot, QolipError> {
        let specs = self.product_specs.read().await;
        let products = self.products.read().await.clone();
        if !products
            .iter()
            .any(|product| product.code.eq_ignore_ascii_case(item_code))
        {
            return Err(QolipError::MissingItem);
        }
        let mut links = self.shared_links.write().await;
        let mut overrides = self.product_overrides.write().await;
        let mut registry = self.managed_registry.write().await;
        let current = memory_shared_snapshot(&specs, &products, &links, &overrides, set_id, &mut registry)?
            .ok_or(QolipError::SetChanged)?;
        if current.members.is_empty() || current.fingerprint != fingerprint {
            return Err(QolipError::SetChanged);
        }
        if current.members.iter().any(|spec| {
            spec.warehouse.trim().is_empty()
                || !allowed
                    .iter()
                    .any(|w| w.eq_ignore_ascii_case(&spec.warehouse))
        }) {
            return Err(QolipError::AccessDenied);
        }
        if !links.iter().any(|(id, _)| id == set_id) {
            for spec in &current.members {
                if !spec.item_code.trim().is_empty() {
                    links.insert((set_id.to_string(), spec.item_code.trim().to_lowercase()));
                }
            }
        }
        links.insert((set_id.to_string(), item_code.trim().to_lowercase()));
        for member in &current.members {
            overrides.remove(&(item_code.trim().to_lowercase(), member.qolip_code.trim().to_lowercase()));
        }
        memory_shared_snapshot(&specs, &products, &links, &overrides, set_id, &mut registry)?.ok_or(QolipError::SetChanged)
    }
}
