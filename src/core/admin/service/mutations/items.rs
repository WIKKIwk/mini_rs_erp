use super::super::helpers::*;
use super::super::*;
use crate::core::admin::item_customer_policy::{
    FINISHED_GOODS_CUSTOMER_REQUIRED, item_group_requires_customer,
};
use crate::core::admin::models::AdminItemGroup;

impl AdminService {
    pub async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
        customer_ref: &str,
    ) -> Result<SupplierItem, AdminPortError> {
        let customer_ref = customer_ref.trim();
        let groups = self.item_group_tree().await?;
        if item_group_requires_customer(item_group, &groups) && customer_ref.is_empty() {
            return Err(AdminPortError::InvalidInput(
                FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
            ));
        }
        if !customer_ref.is_empty() {
            self.read_port()?.customer_by_ref(customer_ref).await?;
        }
        self.write_port()?
            .create_item_with_customer(
                code.trim(),
                name.trim(),
                uom.trim(),
                item_group.trim(),
                (!customer_ref.is_empty()).then_some(customer_ref),
            )
            .await
    }

    pub async fn update_item(
        &self,
        original_code: &str,
        code: &str,
        name: &str,
    ) -> Result<AdminItemDetail, AdminPortError> {
        let original_code = original_code.trim();
        let code = code.trim();
        let name = name.trim();
        if original_code.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "original item code is required".to_string(),
            ));
        }
        if code.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item code is required".to_string(),
            ));
        }
        if name.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item name is required".to_string(),
            ));
        }
        self.write_port()?
            .update_item(original_code, code, name)
            .await
    }

    pub async fn delete_item(&self, code: &str) -> Result<(), AdminPortError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item code is required".to_string(),
            ));
        }
        self.write_port()?.delete_item(code).await
    }

    pub async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item group name is required".to_string(),
            ));
        }
        let parent = if parent.trim().is_empty() {
            "All Item Groups"
        } else {
            parent.trim()
        };
        self.write_port()?
            .create_item_group(name, parent, is_group)
            .await
    }

    pub async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item group name is required".to_string(),
            ));
        }
        if name == "All Item Groups" {
            return Err(AdminPortError::InvalidInput(
                "root item group cannot be moved".to_string(),
            ));
        }
        let parent = if parent.trim().is_empty() {
            "All Item Groups"
        } else {
            parent.trim()
        };
        if name == parent {
            return Err(AdminPortError::InvalidInput(
                "item group cannot be its own parent".to_string(),
            ));
        }
        let groups = self.item_group_tree().await?;
        if item_group_parent_move_would_cycle(name, parent, &groups) {
            return Err(AdminPortError::InvalidInput(
                "item group parent cycle detected".to_string(),
            ));
        }
        self.write_port()?
            .move_item_group_parent(name, parent)
            .await
    }

    pub async fn move_items_to_group(
        &self,
        item_codes: Vec<String>,
        item_group: &str,
    ) -> Result<AdminItemGroupBulkMoveResult, AdminPortError> {
        let codes = normalize_item_codes(item_codes);
        if codes.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item codes are required".to_string(),
            ));
        }
        let group = item_group.trim();
        if group.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "item group is required".to_string(),
            ));
        }
        let groups = self.item_group_tree().await?;
        if item_group_requires_customer(group, &groups) {
            for code in &codes {
                match self.read_port()?.item_detail(code).await {
                    Ok(detail) if detail.customers.is_empty() => {
                        return Err(AdminPortError::InvalidInput(
                            FINISHED_GOODS_CUSTOMER_REQUIRED.to_string(),
                        ));
                    }
                    Ok(_) | Err(AdminPortError::NotFound) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        let stored_updated = self
            .write_port()?
            .update_item_groups_bulk(&codes, group)
            .await?;
        let updated = codes
            .iter()
            .filter(|code| {
                stored_updated
                    .iter()
                    .any(|stored| stored.trim().eq_ignore_ascii_case(code.trim()))
            })
            .cloned()
            .collect::<Vec<_>>();
        let failed = codes
            .iter()
            .filter(|code| {
                !updated
                    .iter()
                    .any(|stored| stored.trim().eq_ignore_ascii_case(code.trim()))
            })
            .cloned()
            .collect::<Vec<_>>();
        Ok(AdminItemGroupBulkMoveResult {
            item_group: group.to_string(),
            requested_count: codes.len(),
            updated_count: updated.len(),
            failed_count: failed.len(),
            updated_item_codes: updated,
            failed_item_codes: failed,
        })
    }
}

fn item_group_parent_move_would_cycle(name: &str, parent: &str, groups: &[AdminItemGroup]) -> bool {
    let name = name.trim();
    let mut current = parent.trim();
    let mut seen = std::collections::BTreeSet::new();
    while !current.is_empty() && seen.insert(current.to_ascii_lowercase()) {
        if current.eq_ignore_ascii_case(name) {
            return true;
        }
        let Some(group) = groups.iter().find(|group| {
            group.item_group_name.trim().eq_ignore_ascii_case(current)
                || group.name.trim().eq_ignore_ascii_case(current)
        }) else {
            return false;
        };
        current = group.parent_item_group.trim();
    }
    false
}
