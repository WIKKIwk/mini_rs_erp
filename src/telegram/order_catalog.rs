use std::collections::BTreeMap;
use std::sync::Arc;

use crate::core::admin::item_customer_policy::FINISHED_GOODS_GROUP;
use crate::core::admin::models::AdminCustomerDetail;
use crate::core::admin::service::AdminService;
use crate::core::calculate_materials::{CalculateMaterial, CalculateMaterialStorePort};
use crate::core::production_map::ProductionMapService;
use crate::core::werka::models::{CustomerDirectoryEntry, SupplierItem};

use super::order::normalize_order_text;

#[derive(Clone)]
pub(crate) struct TelegramOrderCatalog {
    admin: AdminService,
    materials: Arc<dyn CalculateMaterialStorePort>,
    production_maps: ProductionMapService,
}

impl TelegramOrderCatalog {
    pub(crate) fn new(
        admin: AdminService,
        materials: Arc<dyn CalculateMaterialStorePort>,
        production_maps: ProductionMapService,
    ) -> Self {
        Self {
            admin,
            materials,
            production_maps,
        }
    }

    pub(crate) async fn search_customers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CustomerDirectoryEntry>, String> {
        let query = query.trim();
        let query_key = normalize_order_text(query);
        let mut entries = BTreeMap::new();
        for entry in self
            .admin
            .customers_page(query, 100, 0)
            .await
            .map_err(|error| error.to_string())?
        {
            entries.insert(entry.ref_.clone(), entry);
        }
        let mut result = filter_customers(entries.into_values().collect(), &query_key);
        if !query_key.is_empty() && result.is_empty() {
            let fallback = self
                .admin
                .customers_page("", 500, 0)
                .await
                .map_err(|error| error.to_string())?;
            result = filter_customers(fallback, &query_key);
        }
        result.sort_by_key(|entry| normalize_order_text(&entry.name));
        result.truncate(limit.clamp(1, 50));
        Ok(result)
    }

    pub(crate) async fn find_customer_by_name(
        &self,
        name: &str,
    ) -> Result<Option<CustomerDirectoryEntry>, String> {
        let key = normalize_order_text(name);
        if key.is_empty() {
            return Ok(None);
        }
        let mut offset = 0;
        loop {
            let page = self
                .admin
                .customers_page("", 500, offset)
                .await
                .map_err(|error| error.to_string())?;
            let page_len = page.len();
            if let Some(entry) = page
                .into_iter()
                .find(|entry| normalize_order_text(&entry.name) == key)
            {
                return Ok(Some(entry));
            }
            if page_len < 500 {
                return Ok(None);
            }
            offset += page_len;
        }
    }

    pub(crate) async fn customer_by_ref(
        &self,
        customer_ref: &str,
    ) -> Result<CustomerDirectoryEntry, String> {
        let detail: AdminCustomerDetail = self
            .admin
            .customer_detail(customer_ref)
            .await
            .map_err(|error| error.to_string())?;
        Ok(CustomerDirectoryEntry {
            ref_: detail.ref_,
            name: detail.name,
            phone: detail.phone,
        })
    }

    pub(crate) async fn create_customer(
        &self,
        name: &str,
    ) -> Result<CustomerDirectoryEntry, String> {
        self.admin
            .create_customer_name_only(name)
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn search_customer_items(
        &self,
        customer_ref: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SupplierItem>, String> {
        let query_key = normalize_order_text(query);
        let mut items = self
            .admin
            .customer_items(customer_ref, query, 500)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|item| {
                query_key.is_empty()
                    || normalize_order_text(&item.name).contains(&query_key)
                    || normalize_order_text(&item.code).contains(&query_key)
            })
            .collect::<Vec<_>>();
        items.sort_by_key(|item| normalize_order_text(&item.name));
        items.truncate(limit.clamp(1, 50));
        Ok(items)
    }

    pub(crate) async fn customer_item_by_code(
        &self,
        customer_ref: &str,
        item_code: &str,
    ) -> Result<Option<SupplierItem>, String> {
        Ok(self
            .admin
            .customer_items(customer_ref, "", 500)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|item| item.code.trim().eq_ignore_ascii_case(item_code.trim())))
    }

    pub(crate) async fn find_customer_item_by_name(
        &self,
        customer_ref: &str,
        name: &str,
    ) -> Result<Option<SupplierItem>, String> {
        let key = normalize_order_text(name);
        Ok(self
            .admin
            .customer_items(customer_ref, "", 500)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|item| normalize_order_text(&item.name) == key))
    }

    pub(crate) async fn create_product(
        &self,
        customer_ref: &str,
        name: &str,
    ) -> Result<SupplierItem, String> {
        self.admin
            .create_item(name, name, "Kg", FINISHED_GOODS_GROUP, customer_ref)
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn search_materials(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CalculateMaterial>, String> {
        let query_key = normalize_order_text(query);
        let mut materials = self
            .materials
            .list()
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|material| {
                material.active
                    && !material.variants.is_empty()
                    && (query_key.is_empty()
                        || normalize_order_text(&material.name).contains(&query_key))
            })
            .collect::<Vec<_>>();
        materials.sort_by_key(|material| normalize_order_text(&material.name));
        materials.truncate(limit.clamp(1, 50));
        Ok(materials)
    }

    pub(crate) async fn material_by_id(
        &self,
        material_id: &str,
    ) -> Result<Option<CalculateMaterial>, String> {
        Ok(self
            .materials
            .list()
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|material| material.active && material.id == material_id))
    }

    pub(crate) async fn next_order_number(&self) -> Result<String, String> {
        self.production_maps
            .next_order_number()
            .await
            .map_err(|error| error.to_string())
    }
}

fn filter_customers(
    entries: Vec<CustomerDirectoryEntry>,
    query_key: &str,
) -> Vec<CustomerDirectoryEntry> {
    entries
        .into_iter()
        .filter(|entry| {
            query_key.is_empty()
                || normalize_order_text(&entry.name).contains(query_key)
                || normalize_order_text(&entry.ref_).contains(query_key)
        })
        .collect()
}
