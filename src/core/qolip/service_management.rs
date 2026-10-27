impl QolipService {
    pub async fn rename_set(
        &self,
        input: &super::QolipSetRename,
        principal: &Principal,
        is_admin: bool,
    ) -> Result<super::QolipManagedSet, QolipError> {
        let set_id = input.set_id.trim();
        if set_id.is_empty() || set_id.len() > 512 {
            return Err(QolipError::SetChanged);
        }
        let input = super::QolipSetRename {
            set_id: set_id.to_owned(),
            name: super::management::normalize_set_name(&input.name)?,
        };
        let allowed = self.warehouses_for_principal(principal, false).await?;
        self.store
            .rename_set(&input, principal, if is_admin { None } else { Some(&allowed) })
            .await
    }

    pub async fn create_set(
        &self,
        input: &super::QolipSetCreate,
        principal: &Principal,
        is_admin: bool,
    ) -> Result<super::QolipManagedSet, QolipError> {
        super::management::validate_codes(&input.members, &[])?;
        let allowed = self.warehouses_for_principal(principal, is_admin).await?;
        if input.warehouse.trim().is_empty()
            || !allowed.iter().any(|warehouse| warehouse.eq_ignore_ascii_case(input.warehouse.trim()))
        {
            return Err(QolipError::AccessDenied);
        }
        for member in &input.members {
            member.standalone_spec(&input.warehouse, "", principal)?;
        }
        self.store.create_set(input, principal, &allowed).await
    }

    pub async fn managed_sets(
        &self,
        principal: &Principal,
        is_admin: bool,
    ) -> Result<Vec<super::QolipManagedSet>, QolipError> {
        let allowed = self.warehouses_for_principal(principal, false).await?;
        self.store
            .managed_sets(if is_admin { None } else { Some(&allowed) })
            .await
    }

    pub async fn edit_set(
        &self,
        input: &super::QolipSetEdit,
        principal: &Principal,
        is_admin: bool,
    ) -> Result<super::QolipManagedSet, QolipError> {
        if input.set_id.trim().is_empty()
            || input.set_id.len() > 512
            || input.fingerprint.len() != 64
        {
            return Err(QolipError::SetChanged);
        }
        super::management::validate_codes(&input.add, &input.remove)?;
        let allowed = self.warehouses_for_principal(principal, false).await?;
        self.store
            .edit_set(
                input,
                principal,
                if is_admin { None } else { Some(&allowed) },
            )
            .await
    }

    pub async fn edit_product_molds(
        &self,
        input: &super::QolipProductMoldEdits,
        principal: &Principal,
        is_admin: bool,
    ) -> Result<(), QolipError> {
        if input.changes.is_empty() || input.changes.len() > 1000 {
            return Err(QolipError::MissingItem);
        }
        let mut products = BTreeSet::new();
        for change in &input.changes {
            if change.item_code.trim().is_empty()
                || !products.insert(change.item_code.trim().to_lowercase())
            {
                return Err(QolipError::MissingItem);
            }
            super::management::validate_codes(&change.add, &change.remove)?;
        }
        let allowed = self.warehouses_for_principal(principal, false).await?;
        self.store
            .edit_product_molds(
                input,
                principal,
                if is_admin { None } else { Some(&allowed) },
            )
            .await
    }
}
