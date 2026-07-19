use crate::core::admin::models::{AdminItemDetail, AdminItemGroup};
use crate::core::werka::models::{CustomerDirectoryEntry, SupplierItem};

#[derive(sqlx::FromRow)]
pub(super) struct ItemRow {
    code: String,
    name: String,
    uom: String,
    item_group: String,
}

impl ItemRow {
    pub(super) fn into_item(self) -> SupplierItem {
        SupplierItem {
            code: self.code,
            name: self.name,
            uom: self.uom,
            warehouse: String::new(),
            item_group: self.item_group,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct ItemDetailRow {
    code: String,
    name: String,
    uom: String,
    item_group: String,
    created_at_unix: i64,
    updated_at_unix: i64,
}

impl ItemDetailRow {
    pub(super) fn item_group(&self) -> &str {
        &self.item_group
    }

    pub(super) fn into_detail(
        self,
        customers: Vec<CustomerDirectoryEntry>,
        is_finished_goods: bool,
    ) -> AdminItemDetail {
        AdminItemDetail {
            code: self.code,
            name: self.name,
            uom: self.uom,
            item_group: self.item_group,
            is_finished_goods,
            created_at_unix: self.created_at_unix,
            updated_at_unix: self.updated_at_unix,
            customers,
        }
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct ItemCustomerRow {
    customer_ref: String,
    name: String,
    phone: String,
}

impl ItemCustomerRow {
    pub(super) fn into_customer(self) -> CustomerDirectoryEntry {
        CustomerDirectoryEntry {
            ref_: self.customer_ref,
            name: self.name,
            phone: self.phone,
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
