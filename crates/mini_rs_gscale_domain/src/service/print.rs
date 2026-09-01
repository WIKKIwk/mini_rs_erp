use tokio::sync::oneshot;

use super::error::completed_print;
use super::jobs::{NormalizedMaterialReceiptJob, NormalizedProgressLabelJob};
use super::recording::{record_confirmed_material_receipt, record_parallel_material_receipt};
use super::{
    GscaleService, GscaleServiceError, LateMaterialReceiptErrorHandler,
    PreparedMaterialReceiptPrint,
};
use crate::models::{
    MaterialReceiptPrintRequest, MaterialReceiptPrintResponse, ProgressLabelPrintRequest,
    ProgressLabelPrintResponse, ScaleDriverPrintResponse,
};

impl GscaleService {
    pub fn prepare_progress_label(
        &self,
        request: ProgressLabelPrintRequest,
    ) -> Result<ProgressLabelPrintResponse, GscaleServiceError> {
        let job = NormalizedProgressLabelJob::from_request(&request)?;
        Ok(progress_label_response(
            "prepared",
            client_printer(&job.printer),
            client_print_mode(&job.print_mode),
            "client_usb_pending".to_string(),
            job,
        ))
    }

    pub fn prepare_material_receipt_print(
        &self,
        request: &MaterialReceiptPrintRequest,
    ) -> Result<PreparedMaterialReceiptPrint, GscaleServiceError> {
        let mut job = NormalizedMaterialReceiptJob::from_request(request)?;
        let print_count = job.print_count;
        job.print_count = 1;
        Ok(PreparedMaterialReceiptPrint {
            job: std::sync::Arc::new(job),
            print_count,
        })
    }

    pub fn prepare_material_receipt_client_print(
        &self,
        request: MaterialReceiptPrintRequest,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        let job = NormalizedMaterialReceiptJob::from_request(&request)?;
        require_single_material_receipt(&job)?;
        let epc = self.next_epc()?;
        Ok(material_receipt_response(
            epc,
            String::new(),
            "prepared",
            client_printer(&job.printer),
            client_print_mode(&job.print_mode),
            "client_usb_pending".to_string(),
            job,
        ))
    }

    pub async fn confirm_material_receipt_client_print(
        &self,
        request: MaterialReceiptPrintRequest,
        epc: &str,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        let receipt_store = self.receipt_store()?;
        let job = NormalizedMaterialReceiptJob::from_request(&request)?;
        require_single_material_receipt(&job)?;
        let epc = normalize_client_epc(epc)?;
        let draft_name = record_confirmed_material_receipt(
            receipt_store.as_ref(),
            &job,
            epc.clone(),
            self.warehouse_event_handler.clone(),
        )
        .await?;
        Ok(material_receipt_response(
            epc,
            draft_name,
            "printed",
            client_printer(&job.printer),
            client_print_mode(&job.print_mode),
            "USB OK".to_string(),
            job,
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
        let driver = self.driver()?;
        let job = NormalizedProgressLabelJob::from_request(&request)?;
        let print = completed_print(driver.print_material_receipt(job.driver_request()).await)?;
        Ok(progress_label_response(
            "printed",
            print.printer,
            print.mode,
            print.printer_status,
            job,
        ))
    }

    pub async fn print_material_receipt_driver_first_with_late_error(
        &self,
        request: MaterialReceiptPrintRequest,
        late_error: Option<LateMaterialReceiptErrorHandler>,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        let mut job = NormalizedMaterialReceiptJob::from_request(&request)?;
        let print_count = job.print_count;
        job.print_count = 1;
        let job = std::sync::Arc::new(job);
        let mut last_outcome = None;
        for _ in 0..print_count {
            last_outcome = Some(
                self.print_material_receipt_driver_once_job_with_late_error(
                    job.clone(),
                    late_error.clone(),
                )
                .await?,
            );
        }
        let outcome = last_outcome
            .ok_or_else(|| GscaleServiceError::InvalidInput("print_count_required".to_string()))?;
        Ok(outcome.into_response_from_job(job.as_ref(), print_count))
    }

    pub fn material_receipt_print_count(
        request: &MaterialReceiptPrintRequest,
    ) -> Result<u32, GscaleServiceError> {
        Ok(NormalizedMaterialReceiptJob::from_request(request)?.print_count)
    }

    pub async fn print_material_receipt_driver_once_with_late_error(
        &self,
        request: MaterialReceiptPrintRequest,
        late_error: Option<LateMaterialReceiptErrorHandler>,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        let job = std::sync::Arc::new(NormalizedMaterialReceiptJob::from_request(&request)?);
        require_single_material_receipt(job.as_ref())?;
        let outcome = self
            .print_material_receipt_driver_once_job_with_late_error(job.clone(), late_error)
            .await?;
        Ok(outcome.into_response_from_job(job.as_ref(), 1))
    }

    pub async fn print_material_receipt_driver_once_strict(
        &self,
        request: MaterialReceiptPrintRequest,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        let job = NormalizedMaterialReceiptJob::from_request(&request)?;
        require_single_material_receipt(&job)?;
        let outcome = self
            .print_material_receipt_driver_once_job_strict(&job)
            .await?;
        Ok(outcome.into_response(job, 1))
    }

    pub async fn print_prepared_material_receipt_driver_once_strict(
        &self,
        prepared: &PreparedMaterialReceiptPrint,
    ) -> Result<MaterialReceiptPrintResponse, GscaleServiceError> {
        let outcome = self
            .print_material_receipt_driver_once_job_strict(prepared.job.as_ref())
            .await?;
        Ok(outcome.into_response_from_job(prepared.job.as_ref(), 1))
    }

    async fn print_material_receipt_driver_once_job_with_late_error(
        &self,
        job: std::sync::Arc<NormalizedMaterialReceiptJob>,
        late_error: Option<LateMaterialReceiptErrorHandler>,
    ) -> Result<MaterialReceiptPrintOutcome, GscaleServiceError> {
        let receipt_store = self.receipt_store()?;
        let driver = self.driver()?;
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
        let print = match completed_print(
            driver
                .print_material_receipt(job.driver_request(&epc))
                .await,
        ) {
            Ok(print) => print,
            Err(error) => {
                let _ = print_result_tx.send(false);
                return Err(error);
            }
        };
        let _ = print_result_tx.send(true);
        Ok(MaterialReceiptPrintOutcome::from_driver(
            epc,
            String::new(),
            print,
        ))
    }

    async fn print_material_receipt_driver_once_job_strict(
        &self,
        job: &NormalizedMaterialReceiptJob,
    ) -> Result<MaterialReceiptPrintOutcome, GscaleServiceError> {
        let receipt_store = self.receipt_store()?;
        let driver = self.driver()?;
        let epc = self.next_epc()?;
        let print = completed_print(
            driver
                .print_material_receipt(job.driver_request(&epc))
                .await,
        )?;
        let draft_name = record_confirmed_material_receipt(
            receipt_store.as_ref(),
            job,
            epc.clone(),
            self.warehouse_event_handler.clone(),
        )
        .await?;
        Ok(MaterialReceiptPrintOutcome::from_driver(
            epc, draft_name, print,
        ))
    }
}

struct MaterialReceiptPrintOutcome {
    epc: String,
    draft_name: String,
    printer: String,
    print_mode: String,
    printer_status: String,
}

impl MaterialReceiptPrintOutcome {
    fn from_driver(epc: String, draft_name: String, print: ScaleDriverPrintResponse) -> Self {
        Self {
            epc,
            draft_name,
            printer: print.printer,
            print_mode: print.mode,
            printer_status: print.printer_status,
        }
    }

    fn into_response(
        self,
        job: NormalizedMaterialReceiptJob,
        print_count: u32,
    ) -> MaterialReceiptPrintResponse {
        let mut response = material_receipt_response(
            self.epc,
            self.draft_name,
            "printed",
            self.printer,
            self.print_mode,
            self.printer_status,
            job,
        );
        response.print_count = print_count;
        response
    }

    fn into_response_from_job(
        self,
        job: &NormalizedMaterialReceiptJob,
        print_count: u32,
    ) -> MaterialReceiptPrintResponse {
        MaterialReceiptPrintResponse {
            ok: true,
            status: "printed".to_string(),
            draft_name: self.draft_name,
            epc: self.epc,
            item_code: job.item_code.clone(),
            item_name: job.item_name.clone(),
            warehouse: job.warehouse.clone(),
            qty: job.net_qty,
            net_qty: job.net_qty,
            gross_qty: job.gross_qty,
            width_mm: job.width_mm,
            micron: job.micron,
            unit: job.unit.clone(),
            printer: self.printer,
            print_mode: self.print_mode,
            printer_status: self.printer_status,
            print_count,
        }
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

fn progress_label_response(
    status: &str,
    printer: String,
    print_mode: String,
    printer_status: String,
    job: NormalizedProgressLabelJob,
) -> ProgressLabelPrintResponse {
    ProgressLabelPrintResponse {
        ok: true,
        status: status.to_string(),
        qr_payload: job.qr_payload,
        item_code: job.item_code,
        item_name: job.item_name,
        apparatus: job.apparatus,
        apparatus_display_name: job.apparatus_display_name,
        customer_name: job.customer_name,
        executor_name: job.executor_name,
        qty: job.progress_qty,
        gross_qty: job.gross_qty,
        tare_enabled: job.tare_enabled,
        tare_kg: job.tare_kg,
        unit: job.unit,
        progress_unit: job.progress_unit,
        label_kind: job.label_kind,
        printer,
        print_mode,
        printer_status,
        print_count: job.print_count,
    }
}

#[allow(clippy::too_many_arguments)]
fn material_receipt_response(
    epc: String,
    draft_name: String,
    status: &str,
    printer: String,
    print_mode: String,
    printer_status: String,
    job: NormalizedMaterialReceiptJob,
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
        width_mm: job.width_mm,
        micron: job.micron,
        unit: job.unit,
        printer,
        print_mode,
        printer_status,
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
