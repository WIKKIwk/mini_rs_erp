use async_trait::async_trait;

use crate::core::auth::models::Principal;

use super::models::{
    QolipBlock, QolipCellQr, QolipCheckout, QolipError, QolipLocation, QolipProduct,
    QolipProductSpec,
};

#[async_trait]
pub trait QolipStorePort: Send + Sync {
    async fn assigned_warehouses(&self, principal: &Principal) -> Result<Vec<String>, QolipError>;
    async fn assigned_blocks(&self, principal: &Principal) -> Result<Vec<QolipBlock>, QolipError>;
    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError>;
    async fn products(
        &self,
        query: &str,
        limit: usize,
        with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError>;
    async fn product_spec(&self, item_code: &str) -> Result<Option<QolipProductSpec>, QolipError>;
    async fn product_spec_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipProductSpec>, QolipError> {
        let _ = qolip_code;
        Ok(None)
    }
    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError>;
    async fn delete_product_specs(
        &self,
        qolip_codes: &[String],
    ) -> Result<usize, QolipError> {
        let _ = qolip_codes;
        Err(QolipError::StoreFailed)
    }
    async fn locations(&self, block: &str) -> Result<Vec<QolipLocation>, QolipError>;
    async fn location_by_id(&self, location_id: &str) -> Result<Option<QolipLocation>, QolipError>;
    async fn location_by_qolip_code(
        &self,
        qolip_code: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        let _ = qolip_code;
        Ok(None)
    }
    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError>;
    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError>;
    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError>;
    async fn checkouts(
        &self,
        block: Option<&str>,
        allowed_blocks: Option<&[String]>,
        status: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError>;
    async fn open_checkouts_for_worker(
        &self,
        worker_refs: &[String],
        worker_name: &str,
        limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        let refs = worker_refs
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<std::collections::BTreeSet<_>>();
        let worker_name = worker_name.trim().to_ascii_lowercase();
        let mut checkouts = self
            .checkouts(None, None, "open", 10_000)
            .await?
            .into_iter()
            .filter(|checkout| {
                refs.contains(&checkout.issued_to_ref.trim().to_ascii_lowercase())
                    || !worker_name.is_empty()
                        && checkout
                            .issued_to_name
                            .trim()
                            .eq_ignore_ascii_case(&worker_name)
            })
            .collect::<Vec<_>>();
        checkouts.truncate(limit.max(1));
        Ok(checkouts)
    }
    async fn checkout_by_id(&self, checkout_id: &str) -> Result<Option<QolipCheckout>, QolipError>;
    async fn return_checkout(
        &self,
        checkout_id: &str,
        row_letter: &str,
        column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError>;
    async fn move_location(
        &self,
        location_id: &str,
        row_letter: &str,
        column_number: i32,
        quantity: i32,
    ) -> Result<QolipLocation, QolipError>;
    async fn cell_qr_by_payload(&self, qr_payload: &str)
    -> Result<Option<QolipCellQr>, QolipError>;
}
