use time::OffsetDateTime;

use crate::core::admin::item_customer_policy::{
    FINISHED_GOODS_CUSTOMER_REQUIRED, item_group_is_descendant_of, item_group_requires_customer,
};
use crate::core::admin::models::AdminItemGroup;
use crate::core::admin::ports::AdminPortError;
use crate::core::werka::models::SupplierItem;

use super::{
    JsonAdminStore, StoredItemGroup, StoredSupplierItem, blank_default, push_unique, remove_code,
    stored_item_group_is_finished_goods,
};

impl JsonAdminStore {
    pub(super) async fn unassign_customer_item_with_policy(
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
                .filter(|codes| item_code_is_assigned(codes, &item.code))
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

    pub(super) async fn create_item_and_customer_atomic(
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
        if data
            .items
            .keys()
            .any(|candidate| candidate.trim().eq_ignore_ascii_case(code))
        {
            return Err(AdminPortError::InvalidInput(
                "item code already exists".to_string(),
            ));
        }
        if stored_item_group_is_finished_goods(&data, item_group) && customer_ref.is_none() {
            return Err(AdminPortError::InvalidInput(
                FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
            ));
        }
        if customer_ref.is_some_and(|customer_ref| !data.customers.contains_key(customer_ref)) {
            return Err(AdminPortError::NotFound);
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

    pub(super) async fn upsert_item_group_with_customer_policy(
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
            ensure_subtree_has_customers(&data, &group.name, &groups)?;
        }
        data.item_groups = proposed_groups;
        self.persist(&data).await?;
        Ok(AdminItemGroup::from(&group))
    }

    pub(super) async fn move_item_group_parent_with_customer_policy(
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
        ensure_subtree_has_customers(&data, name, &groups)?;
        data.item_groups = proposed_groups;
        self.persist(&data).await?;
        Ok(result)
    }

    pub(super) async fn update_item_groups_bulk_with_customer_policy(
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
                if !data
                    .customer_items
                    .values()
                    .any(|codes| item_code_is_assigned(codes, &item.code))
                {
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

fn ensure_subtree_has_customers(
    data: &super::StoredAdminData,
    root_group: &str,
    groups: &[AdminItemGroup],
) -> Result<(), AdminPortError> {
    for item in data.items.values() {
        if !item_group_is_descendant_of(&item.item_group, root_group, groups)
            || !item_group_requires_customer(&item.item_group, groups)
        {
            continue;
        }
        if !data
            .customer_items
            .values()
            .any(|codes| item_code_is_assigned(codes, &item.code))
        {
            return Err(AdminPortError::InvalidInput(
                FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
            ));
        }
    }
    Ok(())
}

fn item_code_is_assigned(codes: &[String], item_code: &str) -> bool {
    codes
        .iter()
        .any(|code| code.trim().eq_ignore_ascii_case(item_code))
}
