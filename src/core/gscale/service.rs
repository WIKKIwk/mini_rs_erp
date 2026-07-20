mod error;
#[path = "jobs.rs"]
mod jobs;
mod print;
mod recording;
mod stock;

use std::sync::Arc;

use super::epc::GscaleEpcGenerator;
#[cfg(test)]
use super::models::{
    CreateMaterialReceiptDraftInput, MaterialReceiptPrintRequest, ProgressLabelPrintRequest,
    ScaleDriverPrintRequest,
};
use super::ports::{EpcSource, MaterialReceiptStorePort, ScaleDriverPort};

pub use error::GscaleServiceError;

pub(super) const MIN_BATCH_QTY_KG: f64 = 0.100;
pub(super) const MAX_MATERIAL_PRINT_COUNT: u32 = 100;
pub type LateMaterialReceiptErrorHandler = Arc<dyn Fn(String) + Send + Sync>;
pub type WarehouseEventHandler = Arc<dyn Fn(String, String) + Send + Sync>;

#[derive(Clone)]
pub struct GscaleService {
    receipt_store: Option<Arc<dyn MaterialReceiptStorePort>>,
    driver: Option<Arc<dyn ScaleDriverPort>>,
    epc: Arc<dyn EpcSource>,
    warehouse_event_handler: Option<WarehouseEventHandler>,
}

impl Default for GscaleService {
    fn default() -> Self {
        Self {
            receipt_store: None,
            driver: None,
            epc: Arc::new(GscaleEpcGenerator::new()),
            warehouse_event_handler: None,
        }
    }
}

impl GscaleService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_receipt_store(mut self, receipt_store: Arc<dyn MaterialReceiptStorePort>) -> Self {
        self.receipt_store = Some(receipt_store);
        self
    }

    #[cfg(test)]
    pub fn receipt_store_configured_for_test(&self) -> bool {
        self.receipt_store.is_some()
    }

    pub fn with_driver(mut self, driver: Arc<dyn ScaleDriverPort>) -> Self {
        self.driver = Some(driver);
        self
    }

    pub fn with_warehouse_event_handler(mut self, handler: WarehouseEventHandler) -> Self {
        self.warehouse_event_handler = Some(handler);
        self
    }

    #[cfg(test)]
    pub fn with_epc_source(mut self, epc: Arc<dyn EpcSource>) -> Self {
        self.epc = epc;
        self
    }

    fn next_epc(&self) -> Result<String, GscaleServiceError> {
        let epc = self.epc.next_epc().trim().to_ascii_uppercase();
        if epc.is_empty() {
            return Err(GscaleServiceError::EpcGenerationFailed);
        }
        Ok(epc)
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod service_tests;
