use async_trait::async_trait;

use crate::core::auth::models::Principal;

use super::models::{
    QolipBlock, QolipCellQr, QolipError, QolipLocation, QolipProduct, QolipProductSpec,
};

#[async_trait]
pub trait QolipStorePort: Send + Sync {
    async fn assigned_warehouses(&self, principal: &Principal) -> Result<Vec<String>, QolipError>;
    async fn assigned_blocks(&self, principal: &Principal) -> Result<Vec<QolipBlock>, QolipError>;
    async fn products(
        &self,
        query: &str,
        limit: usize,
        with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError>;
    async fn product_spec(&self, item_code: &str) -> Result<Option<QolipProductSpec>, QolipError>;
    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError>;
    async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError>;
    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError>;
    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError>;
}
