use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::core::admin::models::{
    AdminDirectoryEntry, AdminItemDetail, AdminItemGroup, AdminState, AdminWarehouse,
};
use crate::core::auth::ports::AuthConfigSink;
use crate::core::werka::models::SupplierItem;

#[async_trait]
pub trait AdminReadPort: Send + Sync {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError>;

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError>;

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError>;

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError>;

    async fn material_taminotchilar_page(
        &self,
        _query: &str,
        _limit: usize,
        _offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn material_taminotchi_by_ref(
        &self,
        _ref_: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError>;

    async fn item_uoms(&self) -> Result<Vec<String>, AdminPortError> {
        const PAGE_SIZE: usize = 500;
        let mut uoms = Vec::new();
        let mut offset = 0;
        loop {
            let page = self.items_page("", PAGE_SIZE, offset).await?;
            let page_len = page.len();
            uoms.extend(page.into_iter().map(|item| item.uom));
            if page_len < PAGE_SIZE {
                break;
            }
            offset += page_len;
        }
        Ok(uoms)
    }

    async fn items_page_by_group(
        &self,
        group: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let group = group.trim();
        if group.is_empty() {
            return self.items_page(query, limit, offset).await;
        }
        let mut result = Vec::new();
        let page_size = limit.clamp(20, 500);
        let mut scan_offset = offset;
        while result.len() < limit {
            let page = self.items_page(query, page_size, scan_offset).await?;
            if page.is_empty() {
                break;
            }
            let page_len = page.len();
            result.extend(
                page.into_iter()
                    .filter(|item| item.item_group.trim() == group)
                    .take(limit - result.len()),
            );
            if page_len < page_size {
                break;
            }
            scan_offset += page_len;
        }
        Ok(result)
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError>;

    async fn item_detail(&self, _item_code: &str) -> Result<AdminItemDetail, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError>;

    async fn warehouses(
        &self,
        _query: &str,
        _parent: &str,
        _limit: usize,
    ) -> Result<Vec<AdminWarehouse>, AdminPortError> {
        Ok(Vec::new())
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        let groups = self.item_groups("", 500).await?;
        Ok(groups
            .into_iter()
            .filter_map(|name| {
                let name = name.trim();
                if name.is_empty() {
                    return None;
                }
                Some(AdminItemGroup {
                    name: name.to_string(),
                    item_group_name: name.to_string(),
                    parent_item_group: String::new(),
                    is_group: true,
                })
            })
            .collect())
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError>;

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError>;
}

#[async_trait]
pub trait AdminStatePort: Send + Sync {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError>;
    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError>;
}

#[async_trait]
pub trait AdminEnvPersister: Send + Sync {
    fn upsert(&self, values: BTreeMap<&'static str, String>) -> Result<(), AdminPortError>;
}

pub trait AdminAuthConfigSink: AuthConfigSink {}

impl<T> AdminAuthConfigSink for T where T: AuthConfigSink {}

#[async_trait]
pub trait AdminWritePort: Send + Sync {
    async fn create_supplier(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError>;

    async fn update_supplier_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError>;

    async fn assign_supplier_item(&self, ref_: &str, item_code: &str)
    -> Result<(), AdminPortError>;

    async fn unassign_supplier_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError>;

    async fn create_customer(
        &self,
        name: &str,
        phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError>;

    async fn update_customer_phone(&self, ref_: &str, phone: &str) -> Result<(), AdminPortError>;

    async fn update_customer_code(&self, ref_: &str, code: &str) -> Result<(), AdminPortError>;

    async fn create_material_taminotchi(
        &self,
        _name: &str,
        _phone: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn update_material_taminotchi_phone(
        &self,
        _ref_: &str,
        _phone: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn update_material_taminotchi_code(
        &self,
        _ref_: &str,
        _code: &str,
    ) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn assign_customer_item(&self, ref_: &str, item_code: &str)
    -> Result<(), AdminPortError>;

    async fn unassign_customer_item(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError>;

    async fn unassign_customer_item_guarded(
        &self,
        ref_: &str,
        item_code: &str,
    ) -> Result<(), AdminPortError> {
        self.unassign_customer_item(ref_, item_code).await
    }

    async fn create_item(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
    ) -> Result<SupplierItem, AdminPortError>;

    async fn create_item_with_customer(
        &self,
        code: &str,
        name: &str,
        uom: &str,
        item_group: &str,
        customer_ref: Option<&str>,
    ) -> Result<SupplierItem, AdminPortError> {
        let item = self.create_item(code, name, uom, item_group).await?;
        if let Some(customer_ref) = customer_ref.filter(|value| !value.trim().is_empty()) {
            self.assign_customer_item(customer_ref, &item.code).await?;
        }
        Ok(item)
    }

    async fn update_item(
        &self,
        _original_code: &str,
        _code: &str,
        _name: &str,
    ) -> Result<AdminItemDetail, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn delete_item(&self, _code: &str) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn create_item_group(
        &self,
        name: &str,
        parent: &str,
        is_group: bool,
    ) -> Result<AdminItemGroup, AdminPortError>;

    async fn move_item_group_parent(
        &self,
        name: &str,
        parent: &str,
    ) -> Result<AdminItemGroup, AdminPortError>;

    async fn update_item_group(
        &self,
        item_code: &str,
        item_group: &str,
    ) -> Result<(), AdminPortError>;

    async fn update_item_groups_bulk(
        &self,
        item_codes: &[String],
        item_group: &str,
    ) -> Result<Vec<String>, AdminPortError> {
        let mut updated = Vec::new();
        for item_code in item_codes {
            if self.update_item_group(item_code, item_group).await.is_ok() {
                updated.push(item_code.clone());
            }
        }
        Ok(updated)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AdminPortError {
    #[error("not found")]
    NotFound,
    #[cfg(test)]
    #[error("permission denied")]
    PermissionDenied,
    #[error("lookup failed")]
    LookupFailed,
    #[error("code regenerate cooldown")]
    CodeRegenCooldown,
    #[error("{0}")]
    InvalidInput(String),
}
