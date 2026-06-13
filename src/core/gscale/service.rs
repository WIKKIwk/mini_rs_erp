use std::sync::Arc;

use tokio::sync::oneshot;

use super::epc::GscaleEpcGenerator;
use super::models::{
    CreateMaterialReceiptDraftInput, MaterialReceiptPrintRequest, MaterialReceiptPrintResponse,
    ScaleDriverPrintRequest, ScaleDriverPrintResponse,
};
use super::ports::{EpcSource, MaterialReceiptErpPort, ScaleDriverPort};

const MIN_BATCH_QTY_KG: f64 = 0.100;
pub type LateMaterialReceiptErrorHandler = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Clone)]
pub struct GscaleService {
    erp: Option<Arc<dyn MaterialReceiptErpPort>>,
    driver: Option<Arc<dyn ScaleDriverPort>>,
    epc: Arc<dyn EpcSource>,
}

impl Default for GscaleService {
    fn default() -> Self {
        Self {
            erp: None,
            driver: None,
            epc: Arc::new(GscaleEpcGenerator::new()),
        }
    }
}

impl GscaleService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_erp(mut self, erp: Arc<dyn MaterialReceiptErpPort>) -> Self {
        self.erp = Some(erp);
        self
    }

    #[cfg(test)]
    pub fn erp_configured_for_test(&self) -> bool {
        self.erp.is_some()
    }

    pub fn with_driver(mut self, driver: Arc<dyn ScaleDriverPort>) -> Self {
        self.driver = Some(driver);
        self
    }

    #[cfg(test)]
    pub fn with_epc_source(mut self, epc: Arc<dyn EpcSource>) -> Self {
        self.epc = epc;
        self
    }

    pub async fn print_material_receipt_driver_first(
        &self,
        request: MaterialReceiptPrintRequest,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        self.print_material_receipt_driver_first_with_late_error(request, None)
            .await
    }

    pub async fn print_material_receipt_driver_first_with_late_error(
        &self,
        request: MaterialReceiptPrintRequest,
        late_error: Option<LateMaterialReceiptErrorHandler>,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        let erp = self.erp.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured("material receipt erp is not configured".to_string())
        })?;
        let driver = self.driver.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured("scale driver is not configured".to_string())
        })?;
        let job = NormalizedMaterialReceiptJob::from_request(request)?;
        let epc = self.next_epc()?;
        let (print_result_tx, print_result_rx) = oneshot::channel();
        tokio::spawn(record_parallel_material_receipt(
            erp.clone(),
            job.clone(),
            epc.clone(),
            print_result_rx,
            late_error,
        ));
        let print = driver
            .print_material_receipt(job.driver_request(&epc))
            .await;
        let print = match print {
            Ok(print) if print_done(&print) => print,
            Ok(print) => {
                let _ = print_result_tx.send(false);
                return Err(GscaleServiceError::PrintFailed {
                    detail: print_error_detail(&print),
                    delete_error: None,
                });
            }
            Err(error) => {
                let _ = print_result_tx.send(false);
                return Err(GscaleServiceError::PrintFailed {
                    detail: error.message(),
                    delete_error: None,
                });
            }
        };
        let _ = print_result_tx.send(true);

        Ok(MaterialReceiptPrintResponse {
            ok: true,
            status: "printed".to_string(),
            draft_name: String::new(),
            epc,
            item_code: job.item_code,
            item_name: job.item_name,
            warehouse: job.warehouse,
            qty: job.net_qty,
            net_qty: job.net_qty,
            gross_qty: job.gross_qty,
            unit: job.unit,
            printer: print.printer,
            print_mode: print.mode,
            printer_status: print.printer_status,
            print_count: job.print_count,
        })
    }
    fn next_epc(&self) -> Result<String, GscaleServiceError> {
        let epc = self.epc.next_epc().trim().to_ascii_uppercase();
        if epc.is_empty() {
            return Err(GscaleServiceError::EpcGenerationFailed);
        }
        Ok(epc)
    }
}

async fn record_parallel_material_receipt(
    erp: Arc<dyn MaterialReceiptErpPort>,
    job: NormalizedMaterialReceiptJob,
    epc: String,
    print_result_rx: oneshot::Receiver<bool>,
    late_error: Option<LateMaterialReceiptErrorHandler>,
) {
    if let Err(error) = record_parallel_material_receipt_inner(erp, job, epc, print_result_rx).await
    {
        tracing::warn!(%error, "RPS batch ERP record failed after driver print");
        if let Some(handler) = late_error {
            handler(error.to_string());
        }
    }
}

async fn record_parallel_material_receipt_inner(
    erp: Arc<dyn MaterialReceiptErpPort>,
    job: NormalizedMaterialReceiptJob,
    epc: String,
    print_result_rx: oneshot::Receiver<bool>,
) -> Result<(), GscaleServiceError> {
    let draft = create_material_receipt_draft(erp.as_ref(), &job, epc).await?;
    let print_ok = print_result_rx.await.unwrap_or(false);
    if !print_ok {
        erp.delete_stock_entry_draft(&draft.name)
            .await
            .map_err(|error| GscaleServiceError::ErpWrite(error.message()))?;
        return Ok(());
    }
    erp.submit_stock_entry_draft(&draft.name)
        .await
        .map_err(|error| GscaleServiceError::SubmitFailed(clean_erp_error(&error.message())))
}

async fn create_material_receipt_draft(
    erp: &dyn MaterialReceiptErpPort,
    job: &NormalizedMaterialReceiptJob,
    epc: String,
) -> Result<super::models::MaterialReceiptDraft, GscaleServiceError> {
    let input = CreateMaterialReceiptDraftInput {
        item_code: job.item_code.clone(),
        warehouse: job.warehouse.clone(),
        qty: job.net_qty,
        barcode: epc,
    };
    erp.create_material_receipt_draft(input)
        .await
        .map_err(|error| GscaleServiceError::ErpWrite(error.message()))
}

#[derive(Debug, Clone, PartialEq)]
struct NormalizedMaterialReceiptJob {
    driver_url: String,
    item_code: String,
    item_name: String,
    warehouse: String,
    printer: String,
    print_mode: String,
    gross_qty: f64,
    net_qty: f64,
    unit: String,
    tare_enabled: bool,
    tare_kg: f64,
    print_count: u32,
}

impl NormalizedMaterialReceiptJob {
    fn from_request(request: MaterialReceiptPrintRequest) -> Result<Self, GscaleServiceError> {
        let item_code = request.item_code.trim().to_string();
        let warehouse = request.warehouse.trim().to_string();
        if item_code.is_empty() || warehouse.is_empty() {
            return Err(GscaleServiceError::InvalidInput(
                "item_code_and_warehouse_required".to_string(),
            ));
        }
        let gross_qty = request.gross_qty;
        if !gross_qty.is_finite() || gross_qty < MIN_BATCH_QTY_KG {
            return Err(GscaleServiceError::InvalidInput(format!(
                "QTY juda kichik: {gross_qty:.3} kg | min {MIN_BATCH_QTY_KG:.3} kg"
            )));
        }
        let tare_enabled = request.tare_enabled || request.tare_kg > 0.0;
        let tare_kg = if tare_enabled && request.tare_kg > 0.0 {
            request.tare_kg
        } else {
            0.0
        };
        let net_qty = if tare_kg > 0.0 {
            (gross_qty - tare_kg).max(0.0)
        } else {
            gross_qty
        };
        if net_qty < MIN_BATCH_QTY_KG {
            return Err(GscaleServiceError::InvalidInput(format!(
                "NETTO juda kichik: brutto {gross_qty:.3} kg - babina {tare_kg:.3} kg = {net_qty:.3} kg | min {MIN_BATCH_QTY_KG:.3} kg"
            )));
        }
        let item_name = blank_default(&request.item_name, &item_code);
        Ok(Self {
            driver_url: request.driver_url.trim().to_string(),
            item_code,
            item_name,
            warehouse,
            printer: request.printer.trim().to_ascii_lowercase(),
            print_mode: request.print_mode.trim().to_ascii_lowercase(),
            gross_qty,
            net_qty,
            unit: blank_default(&request.unit, "kg"),
            tare_enabled: tare_kg > 0.0,
            tare_kg,
            print_count: normalize_print_count(request.print_count),
        })
    }

    fn driver_request(&self, epc: &str) -> ScaleDriverPrintRequest {
        ScaleDriverPrintRequest {
            driver_url: self.driver_url.clone(),
            epc: epc.trim().to_ascii_uppercase(),
            item_code: self.item_code.clone(),
            item_name: self.item_name.clone(),
            warehouse: self.warehouse.clone(),
            printer: self.printer.clone(),
            print_mode: self.print_mode.clone(),
            gross_qty: self.gross_qty,
            unit: self.unit.clone(),
            tare_enabled: self.tare_enabled,
            tare_kg: self.tare_kg,
            print_count: self.print_count,
        }
    }
}

fn normalize_print_count(value: u32) -> u32 {
    if value == 0 { 1 } else { value }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GscaleServiceError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not configured: {0}")]
    NotConfigured(String),
    #[error("epc generation failed")]
    EpcGenerationFailed,
    #[error("erp write failed: {0}")]
    ErpWrite(String),
    #[error("print failed: {detail}")]
    PrintFailed {
        detail: String,
        delete_error: Option<String>,
    },
    #[error("submit failed: {0}")]
    SubmitFailed(String),
}

impl GscaleServiceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::NotConfigured(_) => "gscale_not_configured",
            Self::EpcGenerationFailed => "epc_generation_failed",
            Self::ErpWrite(_) => "erp_write_failed",
            Self::PrintFailed { .. } => "print_failed",
            Self::SubmitFailed(_) => "submit_failed",
        }
    }
}

fn print_done(print: &ScaleDriverPrintResponse) -> bool {
    print.ok && print.status.trim().eq_ignore_ascii_case("done")
}

fn print_error_detail(print: &ScaleDriverPrintResponse) -> String {
    for value in [&print.detail, &print.error, &print.status] {
        let value = value.trim();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    "print failed".to_string()
}

fn clean_erp_error(message: &str) -> String {
    message
        .trim()
        .strip_prefix("erp write failed: ")
        .unwrap_or_else(|| message.trim())
        .to_string()
}

fn blank_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod service_tests;
