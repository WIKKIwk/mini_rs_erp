use std::sync::Arc;

use crate::core::gscale::models::{ScaleDriverPrintRequest, ScaleDriverPrintResponse};
use crate::core::gscale::ports::{EpcSource, ScaleDriverPort};

use super::models::{
    CreateRezkaRepackDraftInput, RezkaOutputLabel, RezkaSourceEntry, RezkaSplitRequest,
    RezkaSplitResponse,
};
use super::ports::{RezkaPortError, RezkaRepackStorePort};

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

    pub async fn split(
        &self,
        source: RezkaSourceEntry,
        request: RezkaSplitRequest,
    ) -> Result<RezkaSplitResponse, RezkaServiceError> {
        let repack_store = self.repack_store.as_ref().ok_or_else(|| {
            RezkaServiceError::NotConfigured("rezka repack store is not configured".into())
        })?;
        let driver = self.driver.as_ref().ok_or_else(|| {
            RezkaServiceError::NotConfigured("scale driver is not configured".into())
        })?;
        let job = NormalizedRezkaSplit::from_request(source, request, self.epc.as_deref())?;
        tracing::info!(
            source_barcode = %job.source.barcode,
            source_item_code = %job.source.item_code,
            source_qty = job.source.qty,
            output_count = job.outputs.len(),
            printable_count = job.printable_outputs.len(),
            outputs = ?rezka_output_log(&job.outputs),
            printable_outputs = ?rezka_output_log(&job.printable_outputs),
            "rezka split normalized"
        );
        let draft = repack_store
            .create_rezka_repack_draft(CreateRezkaRepackDraftInput {
                source: job.source.clone(),
                reason: job.reason.clone(),
                outputs: job.outputs.clone(),
            })
            .await
            .map_err(|error| RezkaServiceError::StoreWrite(error.message()))?;

        for output in &job.printable_outputs {
            tracing::info!(
                stock_entry_name = %draft.name,
                epc = %output.epc,
                item_code = %output.item_code,
                item_name = %output.item_name,
                qty = output.qty,
                uom = %output.uom,
                warehouse = %output.warehouse,
                reason = %output.reason,
                print_qr = output.print_qr,
                "rezka split sending print request"
            );
            let print = driver
                .print_material_receipt(job.driver_request(output))
                .await;
            match print {
                Ok(print) if print_done(&print) => {
                    tracing::info!(
                        stock_entry_name = %draft.name,
                        epc = %output.epc,
                        item_code = %output.item_code,
                        qty = output.qty,
                        printer = %print.printer,
                        mode = %print.mode,
                        status = %print.status,
                        "rezka split print done"
                    );
                }
                Ok(print) => {
                    tracing::warn!(
                        stock_entry_name = %draft.name,
                        epc = %output.epc,
                        item_code = %output.item_code,
                        qty = output.qty,
                        status = %print.status,
                        detail = %print_error_detail(&print),
                        "rezka split print failed"
                    );
                    let _ = repack_store.delete_rezka_repack_draft(&draft.name).await;
                    return Err(RezkaServiceError::PrintFailed(print_error_detail(&print)));
                }
                Err(error) => {
                    tracing::warn!(
                        stock_entry_name = %draft.name,
                        epc = %output.epc,
                        item_code = %output.item_code,
                        qty = output.qty,
                        error = %error.message(),
                        "rezka split print request error"
                    );
                    let _ = repack_store.delete_rezka_repack_draft(&draft.name).await;
                    return Err(RezkaServiceError::PrintFailed(error.message()));
                }
            }
        }

        repack_store
            .submit_rezka_repack_draft(&draft.name)
            .await
            .map_err(|error| {
                RezkaServiceError::SubmitFailed(clean_store_error(&error.message()))
            })?;

        Ok(RezkaSplitResponse {
            ok: true,
            status: "printed".to_string(),
            stock_entry_name: draft.name,
            source_barcode: job.source.barcode,
            outputs: job.printable_outputs,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct NormalizedRezkaSplit {
    source: RezkaSourceEntry,
    reason: String,
    driver_url: String,
    printer: String,
    print_mode: String,
    outputs: Vec<RezkaOutputLabel>,
    printable_outputs: Vec<RezkaOutputLabel>,
}

impl NormalizedRezkaSplit {
    fn from_request(
        source: RezkaSourceEntry,
        request: RezkaSplitRequest,
        epc: Option<&dyn EpcSource>,
    ) -> Result<Self, RezkaServiceError> {
        if source.barcode.trim().is_empty()
            || source.item_code.trim().is_empty()
            || source.warehouse.trim().is_empty()
            || source.qty <= 0.0
        {
            return Err(RezkaServiceError::InvalidInput(
                "source_is_invalid".to_string(),
            ));
        }
        if request
            .source_barcode
            .trim()
            .eq_ignore_ascii_case(&source.barcode)
            == false
        {
            return Err(RezkaServiceError::InvalidInput(
                "source_barcode_mismatch".to_string(),
            ));
        }
        if !request.source_stock_entry.trim().is_empty()
            && request.source_stock_entry.trim() != source.stock_entry_name
        {
            return Err(RezkaServiceError::InvalidInput(
                "source_stock_entry_mismatch".to_string(),
            ));
        }
        if request.source_line_index > 0 && request.source_line_index != source.line_index {
            return Err(RezkaServiceError::InvalidInput(
                "source_line_index_mismatch".to_string(),
            ));
        }
        if request.outputs.len() < 2 {
            return Err(RezkaServiceError::InvalidInput(
                "at_least_two_outputs_required".to_string(),
            ));
        }

        let mut total = 0.0;
        let mut outputs = Vec::with_capacity(request.outputs.len());
        let mut printable_outputs = Vec::new();
        for output in request.outputs {
            let target_warehouse = output.target_warehouse.trim().to_string();
            let print_qr =
                output.print_qr && !is_rezka_scrap_output(&target_warehouse, &output.reason);
            let item_code = if print_qr {
                output.item_code.trim().to_string()
            } else {
                blank_default(&output.item_code, &source.item_code)
            };
            if item_code.is_empty()
                || target_warehouse.is_empty()
                || output.qty <= 0.0
                || (print_qr && output.item_code.trim().is_empty())
            {
                return Err(RezkaServiceError::InvalidInput(
                    "output_item_warehouse_qty_required".to_string(),
                ));
            }
            let next_epc = if print_qr {
                let epc = epc.ok_or_else(|| RezkaServiceError::EpcGenerationFailed)?;
                let next_epc = epc.next_epc().trim().to_ascii_uppercase();
                if next_epc.is_empty() {
                    return Err(RezkaServiceError::EpcGenerationFailed);
                }
                next_epc
            } else {
                String::new()
            };
            let output_label = RezkaOutputLabel {
                epc: next_epc,
                item_name: if print_qr {
                    blank_default(&output.item_name, &item_code)
                } else {
                    blank_default(&output.item_name, &source.item_name)
                },
                item_code,
                qty: output.qty,
                uom: blank_default(&output.uom, &source.uom),
                warehouse: target_warehouse,
                reason: output.reason.trim().to_string(),
                print_qr,
            };
            if output_label.print_qr {
                printable_outputs.push(output_label.clone());
            }
            total += output.qty;
            outputs.push(output_label);
        }
        if (total - source.qty).abs() > QTY_TOLERANCE {
            return Err(RezkaServiceError::InvalidInput(format!(
                "output_total_must_equal_source_qty:{total:.3}!={:.3}",
                source.qty
            )));
        }

        Ok(Self {
            source,
            reason: request.reason.trim().to_string(),
            driver_url: request.driver_url.trim().trim_end_matches('/').to_string(),
            printer: blank_default(&request.printer.to_ascii_lowercase(), "zebra"),
            print_mode: blank_default(&request.print_mode.to_ascii_lowercase(), "rfid"),
            outputs,
            printable_outputs,
        })
    }

    fn driver_request(&self, output: &RezkaOutputLabel) -> ScaleDriverPrintRequest {
        ScaleDriverPrintRequest {
            driver_url: self.driver_url.clone(),
            epc: output.epc.clone(),
            item_code: output.item_code.clone(),
            item_name: output.item_name.clone(),
            warehouse: output.warehouse.clone(),
            executor_name: String::new(),
            printer: self.printer.clone(),
            print_mode: self.print_mode.clone(),
            gross_qty: output.qty,
            unit: output.uom.clone(),
            tare_enabled: false,
            tare_kg: 0.0,
            print_count: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RezkaServiceError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not configured: {0}")]
    NotConfigured(String),
    #[error("epc generation failed")]
    EpcGenerationFailed,
    #[error("store write failed: {0}")]
    StoreWrite(String),
    #[error("print failed: {0}")]
    PrintFailed(String),
    #[error("submit failed: {0}")]
    SubmitFailed(String),
}

impl RezkaServiceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::NotConfigured(_) => "rezka_not_configured",
            Self::EpcGenerationFailed => "epc_generation_failed",
            Self::StoreWrite(_) => "store_write_failed",
            Self::PrintFailed(_) => "print_failed",
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

fn is_rezka_scrap_output(warehouse: &str, reason: &str) -> bool {
    let warehouse = warehouse.trim().to_ascii_lowercase();
    let reason = reason.trim().to_ascii_lowercase();
    warehouse == "brak - ombori - a" || reason.contains("brak") || reason.contains("atxot")
}

fn rezka_output_log(outputs: &[RezkaOutputLabel]) -> Vec<String> {
    outputs
        .iter()
        .map(|output| {
            format!(
                "item_code={} item_name={} qty={:.3} uom={} warehouse={} reason={} print_qr={} epc={}",
                output.item_code,
                output.item_name,
                output.qty,
                output.uom,
                output.warehouse,
                output.reason,
                output.print_qr,
                output.epc
            )
        })
        .collect()
}

fn clean_store_error(message: &str) -> String {
    message
        .trim()
        .strip_prefix("store write failed: ")
        .unwrap_or_else(|| message.trim())
        .to_string()
}

fn blank_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim().to_string()
    } else {
        value.to_string()
    }
}

impl From<RezkaPortError> for RezkaServiceError {
    fn from(value: RezkaPortError) -> Self {
        Self::StoreWrite(value.message())
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod service_tests;
