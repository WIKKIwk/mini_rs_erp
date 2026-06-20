use std::sync::Arc;

use crate::core::auth::models::Principal;

use super::models::{
    QolipBlock, QolipCellQr, QolipCellQrInput, QolipError, QolipLocation, QolipLocationUpsert,
    QolipProduct, QolipProductSpec, QolipProductSpecUpsert,
};
use super::normalize::{normalize_cell_qr, normalize_location, normalize_product_spec};
use super::ports::QolipStorePort;

#[derive(Clone)]
pub struct QolipService {
    store: Arc<dyn QolipStorePort>,
}

impl QolipService {
    pub fn new(store: Arc<dyn QolipStorePort>) -> Self {
        Self { store }
    }

    pub async fn assigned_blocks(
        &self,
        principal: &Principal,
    ) -> Result<Vec<QolipBlock>, QolipError> {
        self.store.assigned_blocks(principal).await
    }

    pub async fn assigned_warehouses(
        &self,
        principal: &Principal,
    ) -> Result<Vec<String>, QolipError> {
        self.store.assigned_warehouses(principal).await
    }

    pub async fn products(
        &self,
        query: &str,
        limit: usize,
        with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        self.store
            .products(query, limit.clamp(1, 100), with_qolip_only)
            .await
    }

    pub async fn upsert_product_spec(
        &self,
        input: QolipProductSpecUpsert,
        principal: &Principal,
    ) -> Result<QolipProductSpec, QolipError> {
        let normalized = normalize_product_spec(input, principal)?;
        self.store.put_product_spec(normalized).await
    }

    pub async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        let block = block.trim();
        if block.is_empty() {
            return Err(QolipError::MissingBlock);
        }
        self.store.locations(block).await
    }

    pub async fn upsert_location(
        &self,
        mut input: QolipLocationUpsert,
        principal: &Principal,
    ) -> Result<QolipLocation, QolipError> {
        if input.qolip_code.trim().is_empty() || input.size <= 0 {
            let spec = self
                .store
                .product_spec(&input.item_code)
                .await?
                .ok_or(QolipError::MissingQolipCode)?;
            if input.item_name.trim().is_empty() {
                input.item_name = spec.item_name.clone();
            }
            if input.item_group.trim().is_empty() {
                input.item_group = spec.item_group.clone();
            }
            input.qolip_code = spec.qolip_code;
            input.size = spec.size;
        }
        let normalized = normalize_location(input, principal)?;
        self.store.put_location(normalized).await
    }

    pub async fn cell_qr(
        &self,
        input: QolipCellQrInput,
        principal: &Principal,
    ) -> Result<QolipCellQr, QolipError> {
        let normalized = normalize_cell_qr(input, principal)?;
        self.store.get_or_create_cell_qr(normalized).await
    }
}
