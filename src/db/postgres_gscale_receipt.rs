use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::gscale::models::{CreateMaterialReceiptDraftInput, MaterialReceiptDraft};
use crate::core::gscale::ports::{GscalePortError, MaterialReceiptStorePort};

#[derive(Clone)]
pub struct PostgresGscaleReceiptStore {
    pool: PgPool,
}

impl PostgresGscaleReceiptStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MaterialReceiptStorePort for PostgresGscaleReceiptStore {
    async fn create_material_receipt_draft(
        &self,
        input: CreateMaterialReceiptDraftInput,
    ) -> Result<MaterialReceiptDraft, GscalePortError> {
        let item_code = input.item_code.trim();
        let warehouse = input.warehouse.trim();
        let barcode = input.barcode.trim();
        if item_code.is_empty() || warehouse.is_empty() || barcode.is_empty() || input.qty <= 0.0 {
            return Err(GscalePortError::InvalidInput(
                "item_code_warehouse_barcode_and_qty_required".to_string(),
            ));
        }
        let name = receipt_name(barcode);
        sqlx::query_as::<_, MaterialReceiptRow>(
            "INSERT INTO mini_gscale_receipts (
                 name, status, item_code, warehouse, qty, uom, barcode, payload_json
             )
             VALUES ($1, 'draft', $2, $3, $4, 'kg', $5, $6)
             ON CONFLICT (barcode) DO UPDATE SET
               name = excluded.name,
               status = 'draft',
               item_code = excluded.item_code,
               warehouse = excluded.warehouse,
               qty = excluded.qty,
               uom = excluded.uom,
               payload_json = excluded.payload_json,
               updated_at = now(),
               submitted_at = NULL
             RETURNING name, item_code, warehouse, qty, uom, barcode",
        )
        .bind(name)
        .bind(item_code)
        .bind(warehouse)
        .bind(input.qty)
        .bind(barcode)
        .bind(serde_json::json!({
            "item_code": item_code,
            "warehouse": warehouse,
            "qty": input.qty,
            "uom": "kg",
            "barcode": barcode,
        }))
        .fetch_one(&self.pool)
        .await
        .map(row_to_draft)
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        let affected = sqlx::query(
            "UPDATE mini_gscale_receipts
             SET status = 'submitted', submitted_at = now(), updated_at = now()
             WHERE name = $1 AND status = 'draft'",
        )
        .bind(name.trim())
        .execute(&self.pool)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?
        .rows_affected();
        if affected == 0 {
            return Err(GscalePortError::StoreWrite(
                "material receipt draft not found".to_string(),
            ));
        }
        Ok(())
    }

    async fn material_receipt_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<MaterialReceiptDraft>, GscalePortError> {
        let barcode = barcode.trim();
        if barcode.is_empty() {
            return Err(GscalePortError::InvalidInput(
                "barcode is required".to_string(),
            ));
        }
        sqlx::query_as::<_, MaterialReceiptRow>(
            "SELECT name, item_code, warehouse, qty, uom, barcode
             FROM mini_gscale_receipts
             WHERE lower(barcode) = lower($1)
             ORDER BY updated_at DESC
             LIMIT 1",
        )
        .bind(barcode)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(row_to_draft))
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))
    }

    async fn delete_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        sqlx::query("DELETE FROM mini_gscale_receipts WHERE name = $1 AND status = 'draft'")
            .bind(name.trim())
            .execute(&self.pool)
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct MaterialReceiptRow {
    name: String,
    item_code: String,
    warehouse: String,
    qty: f64,
    uom: String,
    barcode: String,
}

fn row_to_draft(row: MaterialReceiptRow) -> MaterialReceiptDraft {
    MaterialReceiptDraft {
        name: row.name,
        item_code: row.item_code,
        warehouse: row.warehouse,
        qty: row.qty,
        uom: row.uom,
        barcode: row.barcode,
    }
}

fn receipt_name(barcode: &str) -> String {
    format!("GSR-{}", barcode.trim().to_ascii_uppercase())
}
