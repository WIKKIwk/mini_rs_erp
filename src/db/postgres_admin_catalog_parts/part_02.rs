
#[async_trait]
impl AdminWritePort for PostgresAdminCatalogStore {
    async fn create_supplier(
        &self,
        _name: &str,
        _phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn update_supplier_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn assign_supplier_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn unassign_supplier_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn create_customer(
        &self,
        _name: &str,
        _phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn update_customer_phone(&self, _ref_: &str, _phone: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn update_customer_code(&self, _ref_: &str, _code: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn assign_customer_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn unassign_customer_item(
        &self,
        _ref_: &str,
        _item_code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        self.insert_item(code, name, uom, item_group).await
    }

    async fn create_item_with_customer(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
        customer_ref: Option<&str>,
    ) -> Result<SupplierItem, AdminPortError> {
        self.insert_item_with_customer(code, name, uom, item_group, customer_ref)
            .await
    }

    async fn update_item(
        &self,
        original_code: &str,
        code: &str,
        name: &str,
    ) -> Result<AdminItemDetail, AdminPortError> {
        self.update_item_identity(original_code, code, name).await
    }

    async fn delete_item(&self, code: &str) -> Result<(), AdminPortError> {
        self.delete_item_safely(code).await
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        self.upsert_item_group(&AdminItemGroup {
            name: name.trim().to_string(),
            item_group_name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group,
        })
        .await
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        lock_item_customer_policy(&mut transaction).await?;
        let affected = sqlx::query(
            "UPDATE mini_item_groups
             SET parent_item_group = $2, updated_at = now()
             WHERE name = $1",
        )
        .bind(name.trim())
        .bind(parent.trim())
        .execute(&mut *transaction)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?
        .rows_affected();
        if affected == 0 {
            return Err(AdminPortError::NotFound);
        }
        let customerless = customerless_items_in_subtree(&mut transaction, name).await?;
        if !customerless.is_empty() {
            return Err(AdminPortError::InvalidInput(
                FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
            ));
        }
        transaction
            .commit()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        Ok(AdminItemGroup {
            name: name.trim().to_string(),
            item_group_name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group: true,
        })
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        let updated = self
            .update_item_groups_bulk_atomic(&[item_code.trim().to_string()], item_group)
            .await?;
        if updated.is_empty() {
            return Err(AdminPortError::NotFound);
        }
        Ok(())
    }

    async fn update_item_groups_bulk(
        &self,
        item_codes: &[String],
        item_group: &str,
    ) -> Result<Vec<String>, AdminPortError> {
        self.update_item_groups_bulk_atomic(item_codes, item_group)
            .await
    }
}
