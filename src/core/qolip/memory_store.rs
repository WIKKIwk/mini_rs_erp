use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

use crate::core::auth::models::Principal;

use super::models::{
    QolipBlock, QolipCellQr, QolipError, QolipLocation, QolipProduct, QolipProductSpec,
};
use super::ports::QolipStorePort;

#[derive(Default)]
pub struct MemoryQolipStore {
    blocks: RwLock<Vec<QolipBlock>>,
    products: RwLock<Vec<QolipProduct>>,
    product_specs: RwLock<BTreeMap<String, QolipProductSpec>>,
    locations: RwLock<Vec<QolipLocation>>,
    cell_qrs: RwLock<BTreeMap<String, QolipCellQr>>,
}

impl MemoryQolipStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub async fn seed_blocks(&self, blocks: Vec<QolipBlock>) {
        *self.blocks.write().await = blocks;
    }

    #[cfg(test)]
    pub async fn seed_products(&self, products: Vec<QolipProduct>) {
        *self.products.write().await = products;
    }
}

#[async_trait]
impl QolipStorePort for MemoryQolipStore {
    async fn assigned_warehouses(&self, _principal: &Principal) -> Result<Vec<String>, QolipError> {
        let mut warehouses = self
            .blocks
            .read()
            .await
            .iter()
            .map(|block| block.warehouse.trim().to_string())
            .filter(|warehouse| !warehouse.is_empty())
            .collect::<Vec<_>>();
        warehouses.sort_by_key(|warehouse| warehouse.to_lowercase());
        warehouses.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        Ok(warehouses)
    }

    async fn assigned_blocks(&self, _principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(self.blocks.read().await.clone())
    }

    async fn products(
        &self,
        query: &str,
        limit: usize,
        with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        let query = query.trim().to_lowercase();
        let specs = self.product_specs.read().await;
        let products = self.products.read().await;
        Ok(products
            .iter()
            .filter_map(|product| {
                let spec = specs.get(&product.code.trim().to_lowercase());
                if with_qolip_only && spec.is_none() {
                    return None;
                }
                let matches = query.is_empty()
                    || product.name.to_lowercase().contains(&query)
                    || product.code.to_lowercase().contains(&query);
                if !matches {
                    return None;
                }
                let mut product = product.clone();
                if let Some(spec) = spec {
                    product.qolip_code = spec.qolip_code.clone();
                    product.size = spec.size;
                    product.has_qolip_spec = true;
                }
                Some(product)
            })
            .take(limit.max(1))
            .collect())
    }

    async fn product_spec(&self, item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        Ok(self
            .product_specs
            .read()
            .await
            .get(&item_code.trim().to_lowercase())
            .cloned())
    }

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        self.product_specs
            .write()
            .await
            .insert(spec.item_code.trim().to_lowercase(), spec.clone());
        Ok(spec)
    }

    async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        let block = block.trim().to_lowercase();
        Ok(self
            .locations
            .read()
            .await
            .iter()
            .filter(|location| location.block.to_lowercase() == block)
            .cloned()
            .collect())
    }

    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError> {
        let mut locations = self.locations.write().await;
        if let Some(index) = locations.iter().position(|item| item.id == location.id) {
            locations[index] = location.clone();
        } else {
            locations.push(location.clone());
        }
        locations.sort_by(|left, right| {
            left.row_letter
                .cmp(&right.row_letter)
                .then_with(|| left.column_number.cmp(&right.column_number))
                .then_with(|| left.item_name.cmp(&right.item_name))
        });
        Ok(location)
    }

    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError> {
        let mut cell_qrs = self.cell_qrs.write().await;
        if let Some(existing) = cell_qrs.get(&cell.id) {
            return Ok(existing.clone());
        }
        cell_qrs.insert(cell.id.clone(), cell.clone());
        Ok(cell)
    }
}
