use tokio::sync::oneshot;

use super::error::{print_done, print_error_detail};
use super::jobs::{NormalizedMaterialReceiptJob, NormalizedProgressLabelJob};
use super::recording::{record_confirmed_material_receipt, record_parallel_material_receipt};
use super::{GscaleService, GscaleServiceError, LateMaterialReceiptErrorHandler};
use crate::core::gscale::models::{
    MaterialReceiptPrintRequest, MaterialReceiptPrintResponse, ProgressLabelPrintRequest,
    ProgressLabelPrintResponse,
};

impl GscaleService {
    pub fn prepare_progress_label(
        &self,
        request: ProgressLabelPrintRequest,
    ) -> Result<ProgressLabelPrintResponse, GscaleServiceError> {
        let job = NormalizedProgressLabelJob::from_request(request)?;
        Ok(ProgressLabelPrintResponse {
            ok: true,
            status: "prepared".to_string(),
            qr_payload: job.qr_payload,
            item_code: job.item_code,
            item_name: job.item_name,
            executor_name: job.executor_name,
            qty: job.progress_qty,
            gross_qty: job.gross_qty,
            unit: job.unit,
            progress_unit: job.progress_unit,
            label_kind: job.label_kind,
            printer: client_printer(&job.printer),
            print_mode: client_print_mode(&job.print_mode),
            printer_status: "client_usb_pending".to_string(),
            print_count: job.print_count,
        })
    }

    pub fn prepare_material_receipt_client_print(
        &self,
        request: MaterialReceiptPrintRequest,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        let job = NormalizedMaterialReceiptJob::from_request(request)?;
        require_single_material_receipt(&job)?;
        let epc = self.next_epc()?;
        Ok(material_receipt_client_response(
            job,
            epc,
            String::new(),
            "prepared",
            "client_usb_pending",
        ))
    }

    pub async fn confirm_material_receipt_client_print(
        &self,
        request: MaterialReceiptPrintRequest,
        epc: &str,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        let receipt_store = self.receipt_store.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured(
                "material receipt store is not configured".to_string(),
            )
        })?;
        let job = NormalizedMaterialReceiptJob::from_request(request)?;
        require_single_material_receipt(&job)?;
        let epc = normalize_client_epc(epc)?;
        let draft_name = record_confirmed_material_receipt(
            receipt_store.clone(),
            &job,
            epc.clone(),
            self.warehouse_event_handler.clone(),
        )
        .await?;
        Ok(material_receipt_client_response(
            job, epc, draft_name, "printed", "USB OK",
        ))
    }

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
            label_kind: job.label_kind,
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
        let print_count = Self::material_receipt_print_count(&request)?;
        let mut last_response = None;
        for _ in 0..print_count {
            let mut single_request = request.clone();
            single_request.print_count = 1;
            last_response = Some(
                self.print_material_receipt_driver_once_with_late_error(
                    single_request,
                    late_error.clone(),
                )
                .await?,
            );
        }
        let mut response = last_response.ok_or_else(|| {
            GscaleServiceError::InvalidInput("print_count_required".to_string())
        })?;
        response.print_count = print_count;
        Ok(response)
    }

    pub fn material_receipt_print_count(
        request: &MaterialReceiptPrintRequest,
    ) -> Result<u32, GscaleServiceError> {
        Ok(NormalizedMaterialReceiptJob::from_request(request.clone())?.print_count)
    }

    pub async fn print_material_receipt_driver_once_with_late_error(
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
        require_single_material_receipt(&job)?;
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

fn require_single_material_receipt(
    job: &NormalizedMaterialReceiptJob,
) -> Result<(), GscaleServiceError> {
    if job.print_count != 1 {
        return Err(GscaleServiceError::InvalidInput(
            "material_receipt_requires_unique_epc_per_print".to_string(),
        ));
    }
    Ok(())
}

fn material_receipt_client_response(
    job: NormalizedMaterialReceiptJob,
    epc: String,
    draft_name: String,
    status: &str,
    printer_status: &str,
) -> MaterialReceiptPrintResponse {
    MaterialReceiptPrintResponse {
        ok: true,
        status: status.to_string(),
        draft_name,
        epc,
        item_code: job.item_code,
        item_name: job.item_name,
        warehouse: job.warehouse,
        qty: job.net_qty,
        net_qty: job.net_qty,
        gross_qty: job.gross_qty,
        unit: job.unit,
        printer: client_printer(&job.printer),
        print_mode: client_print_mode(&job.print_mode),
        printer_status: printer_status.to_string(),
        print_count: job.print_count,
    }
}

fn normalize_client_epc(value: &str) -> Result<String, GscaleServiceError> {
    let epc = value.trim().to_ascii_uppercase();
    if epc.len() != 24
        || !epc.starts_with("30")
        || !epc.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(GscaleServiceError::InvalidInput(
            "client_print_epc_invalid".to_string(),
        ));
    }
    Ok(epc)
}

fn client_printer(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        "godex".to_string()
    } else {
        value
    }
}

fn client_print_mode(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        "label".to_string()
    } else {
        value
    }
}
