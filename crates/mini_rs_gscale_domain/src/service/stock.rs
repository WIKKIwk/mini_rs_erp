use super::error::map_receipt_store_error;
use super::{GscaleService, GscaleServiceError};
use crate::models::{
    MaterialReceiptDraft, RawMaterialStockDeleteInput, RawMaterialStockEntry,
    RawMaterialStockUpdateInput,
};
use mini_rs_domain_types::quantity::positive_erp_quantity;

#[derive(Clone, Copy)]
enum RawMaterialStockMark {
    InUse,
    Consumed,
}

impl GscaleService {
    pub async fn material_receipt_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<MaterialReceiptDraft>, GscaleServiceError> {
        let receipt_store = self.receipt_store()?;
        let barcode = required_barcode(barcode)?;
        receipt_store
            .material_receipt_by_barcode(barcode)
            .await
            .map_err(|error| GscaleServiceError::StoreWrite(error.to_string()))
    }

    pub async fn raw_material_stock_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<RawMaterialStockEntry>, GscaleServiceError> {
        let receipt_store = self.receipt_store()?;
        let barcode = required_barcode(barcode)?;
        receipt_store
            .raw_material_stock_by_barcode(barcode)
            .await
            .map_err(|error| GscaleServiceError::StoreWrite(error.to_string()))
    }

    pub async fn raw_material_stock(
        &self,
        warehouse: &str,
        limit: usize,
    ) -> Result<Vec<RawMaterialStockEntry>, GscaleServiceError> {
        let receipt_store = self.receipt_store()?;
        receipt_store
            .raw_material_stock(warehouse.trim(), limit)
            .await
            .map_err(|error| GscaleServiceError::StoreWrite(error.to_string()))
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
        let receipt_store = self.receipt_store()?;
        receipt_store
            .update_raw_material_stock(input)
            .await
            .map_err(map_receipt_store_error)
    }

    pub async fn soft_delete_raw_material_stock(
        &self,
        mut input: RawMaterialStockDeleteInput,
    ) -> Result<RawMaterialStockEntry, GscaleServiceError> {
        input.barcode = input.barcode.trim().to_string();
        input.expected_warehouse = input.expected_warehouse.trim().to_string();
        if input.barcode.is_empty() || input.expected_warehouse.is_empty() {
            return Err(GscaleServiceError::InvalidInput(
                "raw_material_stock_delete_invalid".to_string(),
            ));
        }
        let receipt_store = self.receipt_store()?;
        receipt_store
            .soft_delete_raw_material_stock(input)
            .await
            .map_err(map_receipt_store_error)
    }

    pub async fn mark_raw_material_stock_in_use(
        &self,
        barcodes: &[String],
        order_id: &str,
    ) -> Result<Vec<RawMaterialStockEntry>, GscaleServiceError> {
        self.mark_raw_material_stock(barcodes, order_id, RawMaterialStockMark::InUse)
            .await
    }

    pub async fn mark_raw_material_stock_consumed(
        &self,
        barcodes: &[String],
        order_id: &str,
    ) -> Result<Vec<RawMaterialStockEntry>, GscaleServiceError> {
        self.mark_raw_material_stock(barcodes, order_id, RawMaterialStockMark::Consumed)
            .await
    }

    async fn mark_raw_material_stock(
        &self,
        barcodes: &[String],
        order_id: &str,
        mark: RawMaterialStockMark,
    ) -> Result<Vec<RawMaterialStockEntry>, GscaleServiceError> {
        let barcodes = barcodes
            .iter()
            .map(|barcode| barcode.trim().to_string())
            .filter(|barcode| !barcode.is_empty())
            .collect::<Vec<_>>();
        if barcodes.is_empty() {
            return Ok(Vec::new());
        }
        let receipt_store = self.receipt_store()?;
        let order_id = order_id.trim();
        if order_id.is_empty() {
            return Err(GscaleServiceError::InvalidInput(
                "order_id is required".to_string(),
            ));
        }
        let result = match mark {
            RawMaterialStockMark::InUse => {
                receipt_store
                    .mark_raw_material_stock_in_use(&barcodes, order_id)
                    .await
            }
            RawMaterialStockMark::Consumed => {
                receipt_store
                    .mark_raw_material_stock_consumed(&barcodes, order_id)
                    .await
            }
        };
        result.map_err(map_receipt_store_error)
    }
}

fn required_barcode(value: &str) -> Result<&str, GscaleServiceError> {
    let value = value.trim();
    if value.is_empty() {
        Err(GscaleServiceError::InvalidInput(
            "barcode is required".to_string(),
        ))
    } else {
        Ok(value)
    }
}
