use std::collections::BTreeSet;

use async_trait::async_trait;

use super::*;

#[async_trait]
impl AdminReadPort for JsonAdminStore {
    async fn suppliers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(paginate(
            data.suppliers
                .values()
                .filter(|entry| entry_matches(entry, query))
                .map(AdminDirectoryEntry::from)
                .collect(),
            limit,
            offset,
        ))
    }

    async fn supplier_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        let data = self.data.lock().await;
        data.suppliers
            .get(ref_.trim())
            .map(AdminDirectoryEntry::from)
            .ok_or(AdminPortError::NotFound)
    }

    async fn customers_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(paginate(
            data.customers
                .values()
                .filter(|entry| entry_matches(entry, query))
                .map(AdminDirectoryEntry::from)
                .collect(),
            limit,
            offset,
        ))
    }

    async fn customer_by_ref(&self, ref_: &str) -> Result<AdminDirectoryEntry, AdminPortError> {
        let data = self.data.lock().await;
        data.customers
            .get(ref_.trim())
            .map(AdminDirectoryEntry::from)
            .ok_or(AdminPortError::NotFound)
    }

    async fn material_taminotchilar_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<AdminDirectoryEntry>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(paginate(
            data.material_taminotchilar
                .values()
                .filter(|entry| entry_matches(entry, query))
                .map(AdminDirectoryEntry::from)
                .collect(),
            limit,
            offset,
        ))
    }

    async fn material_taminotchi_by_ref(
        &self,
        ref_: &str,
    ) -> Result<AdminDirectoryEntry, AdminPortError> {
        let data = self.data.lock().await;
        data.material_taminotchilar
            .get(ref_.trim())
            .map(AdminDirectoryEntry::from)
            .ok_or(AdminPortError::NotFound)
    }

    async fn items_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(paginate(
            data.items
                .values()
                .filter(|item| item_matches(item, query))
                .map(SupplierItem::from)
                .collect(),
            limit,
            offset,
        ))
    }

    async fn items_by_codes(
        &self,
        item_codes: &[String],
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let wanted = item_codes
            .iter()
            .map(|code| code.trim().to_lowercase())
            .collect::<BTreeSet<_>>();
        let data = self.data.lock().await;
        Ok(data
            .items
            .values()
            .filter(|item| wanted.contains(&item.code.trim().to_lowercase()))
            .map(SupplierItem::from)
            .collect())
    }

    async fn item_groups(&self, query: &str, limit: usize) -> Result<Vec<String>, AdminPortError> {
        let needle = query.trim().to_lowercase();
        let data = self.data.lock().await;
        Ok(paginate(
            data.item_groups
                .values()
                .filter(|group| needle.is_empty() || group.name.to_lowercase().contains(&needle))
                .map(|group| group.name.clone())
                .collect(),
            limit,
            0,
        ))
    }

    async fn warehouses(
        &self,
        query: &str,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<AdminWarehouse>, AdminPortError> {
        if !parent.trim().is_empty() {
            return Ok(Vec::new());
        }
        let needle = query.trim().to_lowercase();
        let data = self.data.lock().await;
        let mut seen = BTreeSet::new();
        Ok(paginate(
            data.items
                .values()
                .filter_map(|item| {
                    let warehouse = item.warehouse.trim();
                    if warehouse.is_empty()
                        || !seen.insert(warehouse.to_lowercase())
                        || (!needle.is_empty() && !warehouse.to_lowercase().contains(&needle))
                    {
                        return None;
                    }
                    Some(AdminWarehouse {
                        warehouse: warehouse.to_string(),
                        company: String::new(),
                        is_group: false,
                        parent_warehouse: String::new(),
                    })
                })
                .collect(),
            limit,
            0,
        ))
    }

    async fn item_group_tree(&self) -> Result<Vec<AdminItemGroup>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(data
            .item_groups
            .values()
            .map(AdminItemGroup::from)
            .collect())
    }

    async fn assigned_supplier_items(
        &self,
        supplier_ref: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(assigned_items(
            &data.items,
            data.supplier_items
                .get(supplier_ref.trim())
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            "",
            limit,
        ))
    }

    async fn customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, AdminPortError> {
        let data = self.data.lock().await;
        Ok(assigned_items(
            &data.items,
            data.customer_items
                .get(customer_ref.trim())
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            query,
            limit,
        ))
    }
}
