impl QolipService {
    /// A common production run may use only complete plate sets shared by all
    /// its products, including any required product-local extensions.
    pub async fn required_qolips_for_layout(
        &self,
        products: &[crate::core::order_layout::OrderProduct],
        item_code: &str,
        item_name: &str,
    ) -> Result<Vec<QolipProductSpec>, QolipError> {
        if products.is_empty() { return self.required_qolips_for_order(item_code, item_name).await; }
        let mut all = Vec::new();
        let mut checked = std::collections::BTreeSet::new();
        for product in products {
            if checked.insert(product.item_code.trim()) {
                all.push(self.required_qolips_for_order(&product.item_code, &product.product).await?);
            }
        }
        let first = all.first().ok_or(QolipError::ProductNotFound)?;
        let codes = |specs: Vec<&QolipProductSpec>| specs.into_iter()
            .map(|s| s.qolip_code.trim().to_lowercase()).collect::<std::collections::BTreeSet<_>>();
        let mut accepted = std::collections::BTreeSet::new();
        for candidate in first {
            let selected = selected_product_specs(first, &candidate.qolip_code);
            let expected = codes(selected);
            if all.iter().skip(1).all(|specs| {
                specs.iter().any(|s| s.qolip_code.eq_ignore_ascii_case(&candidate.qolip_code)) &&
                codes(selected_product_specs(specs, &candidate.qolip_code)) == expected
            }) { accepted.extend(expected); }
        }
        if accepted.is_empty() { return Err(QolipError::QolipCodeMismatch); }
        Ok(first.iter().filter(|s| accepted.contains(&s.qolip_code.trim().to_lowercase())).cloned().collect())
    }

    async fn compatible_specs_for_product(
        &self,
        product: &QolipProduct,
    ) -> Result<Vec<QolipProductSpec>, QolipError> {
        let mut specs = self
            .store
            .product_specs(&product.code)
            .await?
            .into_iter()
            .filter(|spec| spec.qolip_set_id.is_empty() && qolip_spec_matches_order(spec, product))
            .collect::<Vec<_>>();
        specs.extend(self.store.linked_product_specs(&product.code).await?);
        specs.retain(|spec| !spec.qolip_code.trim().is_empty());
        specs.sort_by_key(|spec| spec.qolip_code.trim().to_lowercase());
        specs.dedup_by(|a, b| a.qolip_code.eq_ignore_ascii_case(&b.qolip_code));
        Ok(specs)
    }

    pub async fn resolve_shared_set(
        &self,
        identifier: &str,
    ) -> Result<Option<super::QolipSetSnapshot>, QolipError> {
        let identifier = identifier.trim();
        if identifier.is_empty() || identifier.len() > 512 {
            return Err(QolipError::MissingQolipCode);
        }
        self.store.resolve_shared_set(identifier).await
    }

    pub async fn reuse_shared_set(
        &self,
        input: &super::QolipSetReuse,
        principal: &Principal,
        is_admin: bool,
    ) -> Result<super::QolipSetSnapshot, QolipError> {
        if input.set_id.trim().is_empty()
            || input.set_id.len() > 512
            || input.fingerprint.len() != 64
        {
            return Err(QolipError::SetChanged);
        }
        let product = self
            .order_product(&input.item_code, "")
            .await?
            .ok_or(QolipError::MissingItem)?;
        let allowed = if is_admin {
            self.store
                .resolve_shared_set(&input.set_id)
                .await?
                .ok_or(QolipError::SetChanged)?
                .members
                .into_iter()
                .map(|spec| spec.warehouse)
                .collect()
        } else {
            self.warehouses_for_principal(principal, false).await?
        };
        self.store
            .reuse_shared_set(
                &input.set_id,
                &product.code,
                &input.fingerprint,
                principal,
                &allowed,
            )
            .await
    }
}

/// A product uses one alternative base set and every product-local extension.
/// Scanning an extension first keeps the alternative choice open.
pub fn selected_product_specs<'a>(specs: &'a [QolipProductSpec], scanned_code: &str) -> Vec<&'a QolipProductSpec> {
    let selected = specs.iter().find(|spec| spec.qolip_code.eq_ignore_ascii_case(scanned_code));
    let selected_id = selected.filter(|spec| !spec.is_extension).map(|spec| spec.set_id());
    specs.iter().filter(|spec| spec.is_extension || selected_id.as_ref().is_none_or(|id| spec.set_id() == *id)).collect()
}
