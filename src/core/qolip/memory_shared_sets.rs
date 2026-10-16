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
    if targets.is_empty() {
        vec![native.trim().to_lowercase()]
    } else {
        targets
    }
}

fn memory_shared_snapshot(
    specs: &BTreeMap<String, QolipProductSpec>,
    products: &[QolipProduct],
    links: &BTreeSet<(String, String)>,
    identifier: &str,
) -> Result<Option<super::QolipSetSnapshot>, QolipError> {
    let mut groups = BTreeMap::<String, Vec<QolipProductSpec>>::new();
    for spec in specs.values() {
        groups.entry(spec.set_id()).or_default().push(spec.clone());
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
    let display = members
        .iter()
        .max_by_key(|spec| (spec.qolip_code.len(), spec.qolip_code.clone()))
        .map(|spec| spec.qolip_code.clone())
        .unwrap_or_default();
    let targets = memory_set_targets(links, id, &members[0].item_code);
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
        display,
        (*members).clone(),
        products,
    )))
}

impl MemoryQolipStore {
    async fn memory_linked_specs(&self, item_code: &str) -> Vec<QolipProductSpec> {
        let links = self.shared_links.read().await.clone();
        let products = self.products.read().await.clone();
        let group = products
            .iter()
            .find(|p| p.code.eq_ignore_ascii_case(item_code))
            .map(|p| p.item_group.as_str());
        self.product_specs
            .read()
            .await
            .values()
            .filter(|spec| {
                links.contains(&(spec.set_id(), item_code.trim().to_lowercase()))
                    && (!spec.item_code.eq_ignore_ascii_case(item_code)
                        || group.is_some_and(|group| group.eq_ignore_ascii_case(&spec.item_group)))
            })
            .cloned()
            .collect()
    }

    async fn memory_resolve_set(
        &self,
        identifier: &str,
    ) -> Result<Option<super::QolipSetSnapshot>, QolipError> {
        let specs = self.product_specs.read().await.clone();
        let products = self.products.read().await.clone();
        let links = self.shared_links.read().await.clone();
        memory_shared_snapshot(&specs, &products, &links, identifier)
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
        let current = memory_shared_snapshot(&specs, &products, &links, set_id)?
            .ok_or(QolipError::SetChanged)?;
        if current.fingerprint != fingerprint {
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
                links.insert((set_id.to_string(), spec.item_code.trim().to_lowercase()));
            }
        }
        links.insert((set_id.to_string(), item_code.trim().to_lowercase()));
        memory_shared_snapshot(&specs, &products, &links, set_id)?.ok_or(QolipError::SetChanged)
    }
}
