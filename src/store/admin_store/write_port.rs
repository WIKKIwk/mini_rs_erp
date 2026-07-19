use async_trait::async_trait;

use super::*;
use crate::core::admin::item_customer_policy::{
    FINISHED_GOODS_CUSTOMER_REQUIRED, item_group_is_descendant_of, item_group_requires_customer,
};

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
        let mut data = self.data.lock().await;
        let item = data
            .items
            .values()
            .find(|item| item.code.trim().eq_ignore_ascii_case(item_code.trim()))
            .ok_or(AdminPortError::NotFound)?;
        let assigned_to_target = data.customer_items.get(ref_.trim()).is_some_and(|codes| {
            codes
                .iter()
                .any(|code| code.trim().eq_ignore_ascii_case(&item.code))
        });
        if assigned_to_target && stored_item_group_is_finished_goods(&data, &item.item_group) {
            let customer_count = data
                .customer_items
                .values()
                .filter(|codes| {
                    codes
                        .iter()
                        .any(|code| code.trim().eq_ignore_ascii_case(&item.code))
                })
                .count();
            if customer_count <= 1 {
                return Err(AdminPortError::InvalidInput(
                    FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
                ));
            }
        }
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
        let code = code.trim();
        if code.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item code is required".to_string(),
            ));
        }
        let customer_ref = customer_ref
            .map(str::trim)
            .filter(|customer_ref| !customer_ref.is_empty());
        let mut data = self.data.lock().await;
        if stored_item_group_is_finished_goods(&data, item_group) && customer_ref.is_none() {
            return Err(AdminPortError::InvalidInput(
                FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
            ));
        }
        if let Some(customer_ref) = customer_ref {
            if !data.customers.contains_key(customer_ref) {
                return Err(AdminPortError::NotFound);
            }
        }
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let created_at_unix = data
            .items
            .get(code)
            .map(|item| item.created_at_unix)
            .filter(|created_at| *created_at > 0)
            .unwrap_or(now);
        let item = StoredSupplierItem {
            code: code.to_string(),
            name: name.trim().to_string(),
            uom: blank_default(uom, "Kg"),
            item_group: blank_default(item_group, "All Item Groups"),
            created_at_unix,
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
        if let Some(customer_ref) = customer_ref {
            push_unique(
                data.customer_items
                    .entry(customer_ref.to_string())
                    .or_default(),
                code,
            );
        }
        self.persist(&data).await?;
        Ok(SupplierItem::from(&item))
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
        let mut data = self.data.lock().await;
        let group = StoredItemGroup {
            name: name.trim().to_string(),
            parent_item_group: parent.trim().to_string(),
            is_group,
        };
        let existing_groups = data
            .item_groups
            .values()
            .map(AdminItemGroup::from)
            .collect::<Vec<_>>();
        let required_before = data.item_groups.contains_key(&group.name)
            && item_group_requires_customer(&group.name, &existing_groups);
        let mut proposed_groups = data.item_groups.clone();
        proposed_groups.insert(group.name.clone(), group.clone());
        let groups = proposed_groups
            .values()
            .map(AdminItemGroup::from)
            .collect::<Vec<_>>();
        if !required_before && item_group_requires_customer(&group.name, &groups) {
            for item in data.items.values() {
                if !item_group_is_descendant_of(&item.item_group, &group.name, &groups) {
                    continue;
                }
                let has_customer = data.customer_items.values().any(|codes| {
                    codes
                        .iter()
                        .any(|code| code.trim().eq_ignore_ascii_case(&item.code))
                });
                if !has_customer {
                    return Err(AdminPortError::InvalidInput(
                        FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
                    ));
                }
            }
        }
        data.item_groups = proposed_groups;
        self.persist(&data).await?;
        Ok(AdminItemGroup::from(&group))
    }

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let mut data = self.data.lock().await;
        let mut proposed_groups = data.item_groups.clone();
        let group = proposed_groups
            .get_mut(name.trim())
            .ok_or(AdminPortError::NotFound)?;
        group.parent_item_group = parent.trim().to_string();
        let result = AdminItemGroup::from(&*group);
        let groups = proposed_groups
            .values()
            .map(AdminItemGroup::from)
            .collect::<Vec<_>>();
        for item in data.items.values() {
            if !item_group_is_descendant_of(&item.item_group, name, &groups)
                || !item_group_requires_customer(&item.item_group, &groups)
            {
                continue;
            }
            let has_customer = data.customer_items.values().any(|codes| {
                codes
                    .iter()
                    .any(|code| code.trim().eq_ignore_ascii_case(&item.code))
            });
            if !has_customer {
                return Err(AdminPortError::InvalidInput(
                    FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
                ));
            }
        }
        data.item_groups = proposed_groups;
        self.persist(&data).await?;
        Ok(result)
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
        let mut data = self.data.lock().await;
        let groups = data
            .item_groups
            .values()
            .map(AdminItemGroup::from)
            .collect::<Vec<_>>();
        if item_group_requires_customer(item_group, &groups) {
            for item_code in item_codes {
                let Some(item) = data
                    .items
                    .values()
                    .find(|item| item.code.trim().eq_ignore_ascii_case(item_code.trim()))
                else {
                    continue;
                };
                let has_customer = data.customer_items.values().any(|codes| {
                    codes
                        .iter()
                        .any(|code| code.trim().eq_ignore_ascii_case(&item.code))
                });
                if !has_customer {
                    return Err(AdminPortError::InvalidInput(
                        FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
                    ));
                }
            }
        }
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let mut updated = Vec::new();
        for item_code in item_codes {
            let Some(item) = data
                .items
                .values_mut()
                .find(|item| item.code.trim().eq_ignore_ascii_case(item_code.trim()))
            else {
                continue;
            };
            item.item_group = item_group.trim().to_string();
            item.updated_at_unix = now;
            updated.push(item.code.clone());
        }
        self.persist(&data).await?;
        Ok(updated)
    }
}
