#[async_trait]
impl AdminWritePort for AdminCatalogOverlay {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.admin_store.create_supplier(name, phone).await
    }

    async fn update_supplier_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        self.admin_store.update_supplier_phone(ref_, phone).await
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.ensure_assignment_item(item_code).await?;
        self.admin_store.assign_supplier_item(ref_, item_code).await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.admin_store
            .unassign_supplier_item(ref_, item_code)
            .await
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        match &self.customer_store {
            Some(store) => store.create_customer(name, phone).await,
            None => self.admin_store.create_customer(name, phone).await,
        }
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        match &self.customer_store {
            Some(store) => store.update_customer_phone(ref_, phone).await,
            None => self.admin_store.update_customer_phone(ref_, phone).await,
        }
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        self.admin_store.update_customer_code(ref_, code).await
    }

    async fn create_material_taminotchi(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        self.admin_store
            .create_material_taminotchi(name, phone)
            .await
    }

    async fn update_material_taminotchi_phone(
        &self,
        ref_: &str,
        phone: &str,
    ) -> Result<(), AdminPortError> {
        self.admin_store
            .update_material_taminotchi_phone(ref_, phone)
            .await
    }

    async fn update_material_taminotchi_code(
        &self,
        ref_: &str,
        code: &str,
    ) -> Result<(), AdminPortError> {
        self.admin_store
            .update_material_taminotchi_code(ref_, code)
            .await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.ensure_assignment_item(item_code).await?;
        match &self.customer_store {
            Some(store) => store.assign_customer_item(ref_, item_code).await,
            None => self.admin_store.assign_customer_item(ref_, item_code).await,
        }
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.unassign_customer_item_guarded(ref_, item_code).await
    }

    async fn unassign_customer_item_guarded(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        match &self.customer_store {
            Some(store) => store.unassign_customer_item_guarded(ref_, item_code).await,
            None => {
                self.admin_store
                    .unassign_customer_item_guarded(ref_, item_code)
                    .await
            }
        }
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        let item = self
            .catalog
            .create_item(code, name, uom, item_group)
            .await?;
        self.sync_item_to_admin_store(&item).await?;
        Ok(item)
    }

    async fn create_item_with_customer(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
        customer_ref: Option<&str>,
    ) -> Result<SupplierItem, AdminPortError> {
        let item = self
            .catalog
            .create_item_with_customer(code, name, uom, item_group, customer_ref)
            .await?;
        if let Err(error) = self.sync_item_to_admin_store(&item).await {
            tracing::warn!(?error, "item create JSON projection sync failed");
        }
        Ok(item)
    }

    async fn update_item(
        &self,
        original_code: &str,
        code: &str,
        name: &str,
    ) -> Result<AdminItemDetail, AdminPortError> {
        let detail = self.catalog.update_item(original_code, code, name).await?;
        match self
            .admin_store
            .update_item(original_code, &detail.code, &detail.name)
            .await
        {
            Ok(_) => {}
            Err(AdminPortError::NotFound) => {
                let item = SupplierItem {
                    code: detail.code.clone(),
                    name: detail.name.clone(),
                    uom: detail.uom.clone(),
                    warehouse: String::new(),
                    item_group: detail.item_group.clone(),
                    customer_names: Vec::new(),
                };
                if let Err(error) = self.sync_item_to_admin_store(&item).await {
                    tracing::warn!(?error, "item update JSON projection sync failed");
                }
            }
            Err(error) => {
                tracing::warn!(?error, "item update JSON projection sync failed");
            }
        }
        Ok(detail)
    }

    async fn delete_item(&self, code: &str) -> Result<(), AdminPortError> {
        self.catalog.delete_item(code).await?;
        match self.admin_store.delete_item(code).await {
            Ok(()) | Err(AdminPortError::NotFound) => Ok(()),
            Err(error) => {
                tracing::warn!(?error, item_code = %code, "item delete JSON projection sync failed");
                Ok(())
            }
        }
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let group = self
            .catalog
            .create_item_group(name, parent, is_group)
            .await?;
        if let Err(error) = self
            .admin_store
            .create_item_group(&group.name, &group.parent_item_group, group.is_group)
            .await
        {
            tracing::warn!(?error, "item group JSON projection sync failed");
        }
        Ok(group)
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let group = self.catalog.move_item_group_parent(name, parent).await?;
        if let Err(error) = self
            .admin_store
            .move_item_group_parent(&group.name, &group.parent_item_group)
            .await
        {
            tracing::warn!(?error, "item group parent JSON projection sync failed");
        }
        Ok(group)
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        self.catalog.update_item_group(item_code, item_group).await
    }

    async fn update_item_groups_bulk(
        &self,
        item_codes: &[String],
        item_group: &str,
    ) -> Result<Vec<String>, AdminPortError> {
        let updated = self
            .catalog
            .update_item_groups_bulk(item_codes, item_group)
            .await?;
        for item_code in &updated {
            if let Err(error) = self
                .admin_store
                .update_item_group(item_code, item_group)
                .await
            {
                tracing::warn!(
                    ?error,
                    %item_code,
                    "item group JSON projection sync failed"
                );
            }
        }
        Ok(updated)
    }
}
