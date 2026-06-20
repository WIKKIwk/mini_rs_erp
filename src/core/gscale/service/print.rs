use tokio::sync::oneshot;

use super::error::{print_done, print_error_detail};
use super::jobs::{NormalizedMaterialReceiptJob, NormalizedProgressLabelJob};
use super::recording::record_parallel_material_receipt;
use super::{GscaleService, GscaleServiceError, LateMaterialReceiptErrorHandler};
use crate::core::gscale::models::{
    MaterialReceiptPrintRequest, MaterialReceiptPrintResponse, ProgressLabelPrintRequest,
    ProgressLabelPrintResponse,
};

impl GscaleService {
    pub async fn print_material_receipt_driver_first(
        &self,
        request: MaterialReceiptPrintRequest,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        self.print_material_receipt_driver_first_with_late_error(request, None)
            .await
    }

    pub async fn print_progress_label(
        &self,
        request: ProgressLabelPrintRequest,
    ) -> Result<ProgressLabelPrintResponse, GscaleServiceError> {
        let driver = self.driver.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured("scale driver is not configured".to_string())
        })?;
        let job = NormalizedProgressLabelJob::from_request(request)?;
        let print = driver.print_material_receipt(job.driver_request()).await;
        let print = match print {
            Ok(print) if print_done(&print) => print,
            Ok(print) => {
                return Err(GscaleServiceError::PrintFailed {
                    detail: print_error_detail(&print),
                    delete_error: None,
                });
            }
            Err(error) => {
                return Err(GscaleServiceError::PrintFailed {
                    detail: error.message(),
                    delete_error: None,
                });
            }
        };
        Ok(ProgressLabelPrintResponse {
            ok: true,
            status: "printed".to_string(),
            qr_payload: job.qr_payload,
            item_code: job.item_code,
            item_name: job.item_name,
            executor_name: job.executor_name,
            qty: job.progress_qty,
            gross_qty: job.gross_qty,
            unit: job.unit,
            progress_unit: job.progress_unit,
            printer: print.printer,
            print_mode: print.mode,
            printer_status: print.printer_status,
            print_count: job.print_count,
        })
    }

    pub async fn print_material_receipt_driver_first_with_late_error(
        &self,
        request: MaterialReceiptPrintRequest,
        late_error: Option<LateMaterialReceiptErrorHandler>,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        let receipt_store = self.receipt_store.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured(
                "material receipt store is not configured".to_string(),
            )
        })?;
        let driver = self.driver.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured("scale driver is not configured".to_string())
        })?;
        let job = NormalizedMaterialReceiptJob::from_request(request)?;
        let epc = self.next_epc()?;
        let (print_result_tx, print_result_rx) = oneshot::channel();
        tokio::spawn(record_parallel_material_receipt(
            receipt_store.clone(),
            job.clone(),
            epc.clone(),
            print_result_rx,
            late_error,
            self.warehouse_event_handler.clone(),
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
}
