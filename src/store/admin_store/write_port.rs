use async_trait::async_trait;

use super::*;

#[async_trait]
impl AdminWritePort for JsonAdminStore {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        let mut data = self.data.lock().await;
        let ref_ = next_ref("SUP", data.next_supplier_id);
        data.next_supplier_id += 1;
        let entry = AdminDirectoryEntryData::new(&ref_, name, phone);
        data.suppliers.insert(ref_.clone(), entry.clone());
        self.persist(&data).await?;
        Ok(AdminDirectoryEntry::from(&entry))
    }

    async fn update_supplier_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        let entry = data
            .suppliers
            .get_mut(ref_.trim())
            .ok_or(AdminPortError::NotFound)?;
        entry.phone = phone.trim().to_string();
        self.persist(&data).await
    }

    async fn assign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        ensure_item_exists(&data, item_code)?;
        push_unique(
            data.supplier_items
                .entry(ref_.trim().to_string())
                .or_default(),
            item_code,
        );
        self.persist(&data).await
    }

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        remove_code(
            data.supplier_items
                .entry(ref_.trim().to_string())
                .or_default(),
            item_code,
        );
        self.persist(&data).await
    }

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        let mut data = self.data.lock().await;
        let ref_ = next_ref("CUST", data.next_customer_id);
        data.next_customer_id += 1;
        let entry = AdminDirectoryEntryData::new(&ref_, name, phone);
        data.customers.insert(ref_.clone(), entry.clone());
        self.persist(&data).await?;
        Ok(AdminDirectoryEntry::from(&entry))
    }

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        let entry = data
            .customers
            .get_mut(ref_.trim())
            .ok_or(AdminPortError::NotFound)?;
        entry.phone = phone.trim().to_string();
        self.persist(&data).await
    }

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        data.states
            .entry(ref_.trim().to_string())
            .or_default()
            .custom_code = code.trim().to_string();
        self.persist(&data).await
    }

    async fn assign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        ensure_item_exists(&data, item_code)?;
        push_unique(
            data.customer_items
                .entry(ref_.trim().to_string())
                .or_default(),
            item_code,
        );
        self.persist(&data).await
    }

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        remove_code(
            data.customer_items
                .entry(ref_.trim().to_string())
                .or_default(),
            item_code,
        );
        self.persist(&data).await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item code is required".to_string(),
            ));
        }
        let mut data = self.data.lock().await;
        let item = StoredSupplierItem {
            code: code.to_string(),
            name: name.trim().to_string(),
            uom: blank_default(uom, "Kg"),
            warehouse: String::new(),
            item_group: blank_default(item_group, "All Item Groups"),
        };
        data.items.insert(code.to_string(), item.clone());
        data.item_groups
            .entry(item.item_group.clone())
            .or_insert_with(|| StoredItemGroup {
                name: item.item_group.clone(),
                parent_item_group: "All Item Groups".to_string(),
                is_group: true,
            });
        self.persist(&data).await?;
        Ok(SupplierItem::from(&item))
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let mut data = self.data.lock().await;
        let group = StoredItemGroup {
            name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group,
        };
        data.item_groups.insert(group.name.clone(), group.clone());
        self.persist(&data).await?;
        Ok(AdminItemGroup::from(&group))
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let mut data = self.data.lock().await;
        let group = data
            .item_groups
            .get_mut(name.trim())
            .ok_or(AdminPortError::NotFound)?;
        group.parent_item_group = parent.trim().to_string();
        let result = AdminItemGroup::from(&*group);
        self.persist(&data).await?;
        Ok(result)
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        let item = data
            .items
            .get_mut(item_code.trim())
            .ok_or(AdminPortError::NotFound)?;
        item.item_group = item_group.trim().to_string();
        self.persist(&data).await
    }
}
