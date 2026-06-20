mod error;
mod flow;
mod normalize;
mod print;

use std::sync::Arc;

use crate::core::gscale::models::ScaleDriverPrintRequest;
use crate::core::gscale::ports::{EpcSource, ScaleDriverPort};

use super::models::{
    CreateRezkaRepackDraftInput, RezkaOutputLabel, RezkaSourceEntry, RezkaSplitRequest,
    RezkaSplitResponse,
};
use super::ports::RezkaRepackStorePort;

pub use error::RezkaServiceError;
use normalize::NormalizedRezkaSplit;
use print::{clean_store_error, print_done, print_error_detail, rezka_output_log};

const QTY_TOLERANCE: f64 = 0.0001;

#[derive(Clone, Default)]
pub struct RezkaService {
    repack_store: Option<Arc<dyn RezkaRepackStorePort>>,
    driver: Option<Arc<dyn ScaleDriverPort>>,
    epc: Option<Arc<dyn EpcSource>>,
}

impl RezkaService {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn with_repack_store(mut self, repack_store: Arc<dyn RezkaRepackStorePort>) -> Self {
        self.repack_store = Some(repack_store);
        self
    }

    #[cfg(test)]
    pub fn repack_store_configured_for_test(&self) -> bool {
        self.repack_store.is_some()
    }

    pub fn with_driver(mut self, driver: Arc<dyn ScaleDriverPort>) -> Self {
        self.driver = Some(driver);
        self
    }

    pub fn with_epc_source(mut self, epc: Arc<dyn EpcSource>) -> Self {
        self.epc = Some(epc);
        self
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod service_tests;
