use crate::core::admin::models::AdminItemGroup;
use crate::core::werka::models::SupplierItem;

#[derive(sqlx::FromRow)]
pub(super) struct ItemRow {
    code: String,
    name: String,
    uom: String,
    warehouse: String,
    item_group: String,
}

impl ItemRow {
    pub(super) fn into_item(self) -> SupplierItem {
        SupplierItem {
            code: self.code,
            name: self.name,
            uom: self.uom,
            warehouse: self.warehouse,
            item_group: self.item_group,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct ItemGroupRow {
    name: String,
    parent_item_group: String,
    is_group: bool,
}

impl ItemGroupRow {
    pub(super) fn into_group(self) -> AdminItemGroup {
        AdminItemGroup {
            name: self.name.clone(),
            item_group_name: self.name,
            parent_item_group: self.parent_item_group,
            is_group: self.is_group,
        }
    }
}

pub(super) fn blank_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim().to_string()
    } else {
        value.to_string()
    }
}
