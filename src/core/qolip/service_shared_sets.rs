impl QolipService {
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
