use super::error::map_receipt_store_error;
use super::{GscaleService, GscaleServiceError};
use crate::core::gscale::models::{
    MaterialReceiptDraft, RawMaterialStockEntry, RawMaterialStockUpdateInput,
};
use crate::core::quantity::positive_erp_quantity;

impl GscaleService {
    pub async fn material_receipt_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<MaterialReceiptDraft>, GscaleServiceError> {
        let receipt_store = self.receipt_store.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured(
                "material receipt store is not configured".to_string(),
            )
        })?;
        let barcode = barcode.trim();
        if barcode.is_empty() {
            return Err(GscaleServiceError::InvalidInput(
                "barcode is required".to_string(),
            ));
        }
        receipt_store
            .material_receipt_by_barcode(barcode)
            .await
            .map_err(|error| GscaleServiceError::StoreWrite(error.message()))
    }

    pub async fn raw_material_stock_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<RawMaterialStockEntry>, GscaleServiceError> {
        let receipt_store = self.receipt_store.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured(
                "material receipt store is not configured".to_string(),
            )
        })?;
        let barcode = barcode.trim();
        if barcode.is_empty() {
            return Err(GscaleServiceError::InvalidInput(
                "barcode is required".to_string(),
            ));
        }
        receipt_store
            .raw_material_stock_by_barcode(barcode)
            .await
            .map_err(|error| GscaleServiceError::StoreWrite(error.message()))
    }

    pub async fn raw_material_stock(
        &self,
        warehouse: &str,
        limit: usize,
    ) -> Result<Vec<RawMaterialStockEntry>, GscaleServiceError> {
        let receipt_store = self.receipt_store.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured(
                "material receipt store is not configured".to_string(),
            )
        })?;
        receipt_store
            .raw_material_stock(warehouse.trim(), limit)
            .await
            .map_err(|error| GscaleServiceError::StoreWrite(error.message()))
    }

    pub async fn update_raw_material_stock(
        &self,
        mut input: RawMaterialStockUpdateInput,
    ) -> Result<RawMaterialStockEntry, GscaleServiceError> {
        input.barcode = input.barcode.trim().to_string();
        input.item_code = input.item_code.trim().to_string();
        input.item_name = input.item_name.trim().to_string();
        input.qty = positive_erp_quantity(input.qty).ok_or_else(|| {
            GscaleServiceError::InvalidInput("raw_material_stock_qty_invalid".to_string())
        })?;
        if input.barcode.is_empty() || input.item_code.is_empty() || input.item_name.is_empty() {
            return Err(GscaleServiceError::InvalidInput(
                "raw_material_stock_update_invalid".to_string(),
            ));
        }
        let receipt_store = self.receipt_store.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured(
                "material receipt store is not configured".to_string(),
            )
        })?;
        receipt_store
            .update_raw_material_stock(input)
            .await
            .map_err(map_receipt_store_error)
    }

    pub async fn mark_raw_material_stock_in_use(
        &self,
        barcodes: &[String],
        order_id: &str,
    ) -> Result<Vec<RawMaterialStockEntry>, GscaleServiceError> {
        let barcodes = barcodes
            .iter()
            .map(|barcode| barcode.trim().to_string())
            .filter(|barcode| !barcode.is_empty())
            .collect::<Vec<_>>();
        if barcodes.is_empty() {
            return Ok(Vec::new());
        }
        let receipt_store = self.receipt_store.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured(
                "material receipt store is not configured".to_string(),
            )
        })?;
        let order_id = order_id.trim();
        if order_id.is_empty() {
            return Err(GscaleServiceError::InvalidInput(
                "order_id is required".to_string(),
            ));
        }
        receipt_store
            .mark_raw_material_stock_in_use(&barcodes, order_id)
            .await
            .map_err(map_receipt_store_error)
    }

    pub async fn mark_raw_material_stock_consumed(
        &self,
        barcodes: &[String],
        order_id: &str,
    ) -> Result<Vec<RawMaterialStockEntry>, GscaleServiceError> {
        let barcodes = barcodes
            .iter()
            .map(|barcode| barcode.trim().to_string())
            .filter(|barcode| !barcode.is_empty())
            .collect::<Vec<_>>();
        if barcodes.is_empty() {
            return Ok(Vec::new());
        }
        let receipt_store = self.receipt_store.as_ref().ok_or_else(|| {
            GscaleServiceError::NotConfigured(
                "material receipt store is not configured".to_string(),
            )
        })?;
        let order_id = order_id.trim();
        if order_id.is_empty() {
            return Err(GscaleServiceError::InvalidInput(
                "order_id is required".to_string(),
            ));
        }
        receipt_store
            .mark_raw_material_stock_consumed(&barcodes, order_id)
            .await
            .map_err(map_receipt_store_error)
    }
}
