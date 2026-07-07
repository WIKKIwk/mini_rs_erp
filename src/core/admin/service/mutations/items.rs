use super::super::helpers::*;
use super::super::*;
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
        if is_finished_goods_group(item_group) && customer_ref.is_empty() {
            return Err(AdminPortError::InvalidInput(
                "customer_ref is required for tayyor mahsulot".to_string(),
            ));
        }
        if !customer_ref.is_empty() {
            self.read_port()?.customer_by_ref(customer_ref).await?;
        }
        let item = self
            .write_port()?
            .create_item(code.trim(), name.trim(), uom.trim(), item_group.trim())
            .await?;
        if !customer_ref.is_empty() {
            self.write_port()?
                .assign_customer_item(customer_ref, item.code.trim())
                .await?;
        }
        Ok(item)
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
        let mut updated = Vec::new();
        let mut failed = Vec::new();
        for code in &codes {
            if self
                .write_port()?
                .update_item_group(code, group)
                .await
                .is_ok()
            {
                updated.push(code.clone());
            } else {
                failed.push(code.clone());
            }
        }
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

fn is_finished_goods_group(item_group: &str) -> bool {
    item_group.trim().eq_ignore_ascii_case("tayyor mahsulot")
}

fn item_group_parent_move_would_cycle(
    name: &str,
    parent: &str,
    groups: &[AdminItemGroup],
) -> bool {
    let name = name.trim();
    let mut current = parent.trim();
    let mut seen = std::collections::BTreeSet::new();
    while !current.is_empty() && seen.insert(current.to_ascii_lowercase()) {
        if current.eq_ignore_ascii_case(name) {
            return true;
        }
        let Some(group) = groups.iter().find(|group| {
            group
                .item_group_name
                .trim()
                .eq_ignore_ascii_case(current)
                || group.name.trim().eq_ignore_ascii_case(current)
        }) else {
            return false;
        };
        current = group.parent_item_group.trim();
    }
    false
}
