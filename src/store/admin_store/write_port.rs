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

    async fn create_material_taminotchi(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        let mut data = self.data.lock().await;
        let ref_ = next_ref("MAT", data.next_material_taminotchi_id);
        data.next_material_taminotchi_id += 1;
        let entry = AdminDirectoryEntryData::new(&ref_, name, phone);
        data.material_taminotchilar
            .insert(ref_.clone(), entry.clone());
        self.persist(&data).await?;
        Ok(AdminDirectoryEntry::from(&entry))
    }

    async fn update_material_taminotchi_phone(
        &self,
        ref_: &str,
        phone: &str,
    ) -> Result<(), AdminPortError> {
        let mut data = self.data.lock().await;
        let entry = data
            .material_taminotchilar
            .get_mut(ref_.trim())
            .ok_or(AdminPortError::NotFound)?;
        entry.phone = phone.trim().to_string();
        self.persist(&data).await
    }

    async fn update_material_taminotchi_code(
        &self,
        ref_: &str,
        code: &str,
    ) -> Result<(), AdminPortError> {
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
        self.unassign_customer_item_guarded(ref_, item_code).await
    }

    async fn unassign_customer_item_guarded(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.unassign_customer_item_with_policy(ref_, item_code)
            .await
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
        if data
            .items
            .keys()
            .any(|candidate| candidate.trim().eq_ignore_ascii_case(code))
        {
            return Err(AdminPortError::InvalidInput(
                "item code already exists".to_string(),
            ));
        }
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let item = StoredSupplierItem {
            code: code.to_string(),
            name: name.trim().to_string(),
            uom: blank_default(uom, "Kg"),
            item_group: blank_default(item_group, "All Item Groups"),
            created_at_unix: now,
            updated_at_unix: now,
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

    async fn create_item_with_customer(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
        customer_ref: Option<&str>,
    ) -> Result<SupplierItem, AdminPortError> {
        self.create_item_and_customer_atomic(code, name, uom, item_group, customer_ref)
            .await
    }

    async fn update_item(
        &self,
        original_code: &str,
        code: &str,
        name: &str,
    ) -> Result<AdminItemDetail, AdminPortError> {
        let original_code = original_code.trim();
        let code = code.trim();
        let name = name.trim();
        let mut data = self.data.lock().await;
        let original_key = data
            .items
            .keys()
            .find(|candidate| candidate.trim().eq_ignore_ascii_case(original_code))
            .cloned()
            .ok_or(AdminPortError::NotFound)?;
        if data.items.keys().any(|candidate| {
            candidate != &original_key && candidate.trim().eq_ignore_ascii_case(code)
        }) {
            return Err(AdminPortError::InvalidInput(
                "item code already exists".to_string(),
            ));
        }

        let mut item = data
            .items
            .remove(&original_key)
            .ok_or(AdminPortError::NotFound)?;
        item.code = code.to_string();
        item.name = name.to_string();
        if item.created_at_unix <= 0 {
            item.created_at_unix = OffsetDateTime::now_utc().unix_timestamp();
        }
        item.updated_at_unix = OffsetDateTime::now_utc().unix_timestamp();
        replace_assigned_item_code(&mut data.supplier_items, &original_key, code);
        replace_assigned_item_code(&mut data.customer_items, &original_key, code);
        data.items.insert(code.to_string(), item.clone());
        let detail = stored_item_detail(&data, &item);
        self.persist(&data).await?;
        Ok(detail)
    }

    async fn delete_item(&self, code: &str) -> Result<(), AdminPortError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item code is required".to_string(),
            ));
        }
        let mut data = self.data.lock().await;
        let item_key = data
            .items
            .keys()
            .find(|candidate| candidate.trim().eq_ignore_ascii_case(code))
            .cloned()
            .ok_or(AdminPortError::NotFound)?;
        data.items.remove(&item_key);
        for assignments in data.supplier_items.values_mut() {
            remove_code(assignments, &item_key);
        }
        for assignments in data.customer_items.values_mut() {
            remove_code(assignments, &item_key);
        }
        self.persist(&data).await
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        self.upsert_item_group_with_customer_policy(name, parent, is_group)
            .await
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        self.move_item_group_parent_with_customer_policy(name, parent)
            .await
    }

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError> {
        let updated = self
            .update_item_groups_bulk(&[item_code.trim().to_string()], item_group)
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
        self.update_item_groups_bulk_with_customer_policy(item_codes, item_group)
            .await
    }
}
