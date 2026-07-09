use std::sync::Arc;

use tokio::sync::oneshot;

use super::error::clean_store_error;
use super::jobs::NormalizedMaterialReceiptJob;
use super::{GscaleServiceError, LateMaterialReceiptErrorHandler, WarehouseEventHandler};
use crate::core::gscale::models::{CreateMaterialReceiptDraftInput, MaterialReceiptDraft};
use crate::core::gscale::ports::MaterialReceiptStorePort;

pub(super) async fn record_parallel_material_receipt(
    receipt_store: Arc<dyn MaterialReceiptStorePort>,
    job: NormalizedMaterialReceiptJob,
    epc: String,
    print_result_rx: oneshot::Receiver<bool>,
    late_error: Option<LateMaterialReceiptErrorHandler>,
    warehouse_event_handler: Option<WarehouseEventHandler>,
) {
    if let Err(error) = record_parallel_material_receipt_inner(
        receipt_store,
        job,
        epc,
        print_result_rx,
        warehouse_event_handler,
    )
    .await
    {
        tracing::warn!(%error, "RPS batch material receipt record failed after driver print");
        if let Some(handler) = late_error {
            handler(error.to_string());
        }
    }
}

async fn record_parallel_material_receipt_inner(
    receipt_store: Arc<dyn MaterialReceiptStorePort>,
    job: NormalizedMaterialReceiptJob,
    epc: String,
    print_result_rx: oneshot::Receiver<bool>,
    warehouse_event_handler: Option<WarehouseEventHandler>,
) -> Result<(), GscaleServiceError> {
    let draft = create_material_receipt_draft(receipt_store.as_ref(), &job, epc).await?;
    let print_ok = print_result_rx.await.unwrap_or(false);
    if !print_ok {
        receipt_store
            .delete_stock_entry_draft(&draft.name)
            .await
            .map_err(|error| GscaleServiceError::StoreWrite(error.message()))?;
        return Ok(());
    }
    receipt_store
        .submit_stock_entry_draft(&draft.name)
        .await
        .map_err(|error| GscaleServiceError::SubmitFailed(clean_store_error(&error.message())))?;
    if let Some(handler) = warehouse_event_handler {
        handler(job.warehouse, "raw_material_stock".to_string());
    }
    Ok(())
}

async fn create_material_receipt_draft(
    receipt_store: &dyn MaterialReceiptStorePort,
    job: &NormalizedMaterialReceiptJob,
    epc: String,
) -> Result<MaterialReceiptDraft, GscaleServiceError> {
    let input = CreateMaterialReceiptDraftInput {
        item_code: job.item_code.clone(),
        item_name: job.item_name.clone(),
        warehouse: job.warehouse.clone(),
        qty: job.net_qty,
        barcode: epc,
        actor_role: job.actor_role.clone(),
        actor_ref: job.actor_ref.clone(),
        actor_display_name: job.actor_display_name.clone(),
    };
    receipt_store
        .create_material_receipt_draft(input)
        .await
        .map_err(|error| GscaleServiceError::StoreWrite(error.message()))
}
