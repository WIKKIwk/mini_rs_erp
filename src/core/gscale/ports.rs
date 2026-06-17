use async_trait::async_trait;

use super::models::{
    CreateMaterialReceiptDraftInput, MaterialReceiptDraft, ScaleDriverPrintRequest,
    ScaleDriverPrintResponse,
};

#[async_trait]
pub trait MaterialReceiptStorePort: Send + Sync {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError>;

    async fn material_receipt_by_barcode(
        &self,
        _barcode: &str,
    ) -> Result<Option<MaterialReceiptDraft>, GscalePortError> {
        Ok(None)
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError>;

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError>;
}

#[async_trait]
pub trait ScaleDriverPort: Send + Sync {
    async fn print_material_receipt(
        &self,
        request: ScaleDriverPrintRequest,
    ) -> Result<ScaleDriverPrintResponse, GscalePortError>;
}

pub trait EpcSource: Send + Sync {
    fn next_epc(&self) -> String;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GscalePortError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not configured: {0}")]
    NotConfigured(String),
    #[error("store write failed: {0}")]
    StoreWrite(String),
    #[error("driver request failed: {0}")]
    Driver(String),
}

impl GscalePortError {
    pub fn message(&self) -> String {
        self.to_string()
    }
}
