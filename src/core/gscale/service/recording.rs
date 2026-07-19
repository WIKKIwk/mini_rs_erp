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

pub(super) async fn record_confirmed_material_receipt(
    receipt_store: Arc<dyn MaterialReceiptStorePort>,
    job: &NormalizedMaterialReceiptJob,
    epc: String,
    warehouse_event_handler: Option<WarehouseEventHandler>,
) -> Result<String, GscaleServiceError> {
    if let Some(stock) = receipt_store
        .raw_material_stock_by_barcode(&epc)
        .await
        .map_err(|error| GscaleServiceError::StoreWrite(error.message()))?
    {
        validate_existing_receipt(&stock.item_code, &stock.warehouse, stock.qty, job)?;
        return Ok(stock.source_receipt_id);
    }

    let draft = match receipt_store
        .material_receipt_by_barcode(&epc)
        .await
        .map_err(|error| GscaleServiceError::StoreWrite(error.message()))?
    {
        Some(draft) => {
            validate_existing_receipt(&draft.item_code, &draft.warehouse, draft.qty, job)?;
            draft
        }
        None => create_material_receipt_draft(receipt_store.as_ref(), job, epc).await?,
    };

    receipt_store
        .submit_stock_entry_draft(&draft.name)
        .await
        .map_err(|error| GscaleServiceError::SubmitFailed(clean_store_error(&error.message())))?;
    if let Some(handler) = warehouse_event_handler {
        handler(job.warehouse.clone(), "raw_material_stock".to_string());
    }
    Ok(draft.name)
}

fn validate_existing_receipt(
    item_code: &str,
    warehouse: &str,
    qty: f64,
    job: &NormalizedMaterialReceiptJob,
) -> Result<(), GscaleServiceError> {
    if !item_code.trim().eq_ignore_ascii_case(&job.item_code)
        || !warehouse.trim().eq_ignore_ascii_case(&job.warehouse)
        || (qty - job.net_qty).abs() > 0.000_001
    {
        return Err(GscaleServiceError::InvalidInput(
            "client_print_epc_already_used".to_string(),
        ));
    }
    Ok(())
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
