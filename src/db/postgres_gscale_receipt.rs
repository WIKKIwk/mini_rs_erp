use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::gscale::models::{
    CreateMaterialReceiptDraftInput, MaterialReceiptDraft, RawMaterialStockEntry,
};
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
        let row = sqlx::query_as::<_, MaterialReceiptRow>(
            "UPDATE mini_gscale_receipts
             SET status = 'submitted', submitted_at = now(), updated_at = now()
             WHERE name = $1 AND status = 'draft'
             RETURNING name, item_code, warehouse, qty, uom, barcode",
        )
        .bind(name.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let Some(row) = row else {
            return Err(GscalePortError::StoreWrite(
                "material receipt draft not found".to_string(),
            ));
        };
        upsert_raw_material_stock(&self.pool, &row).await?;
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

    async fn raw_material_stock_by_barcode(
        &self,
        barcode: &str,
    ) -> Result<Option<RawMaterialStockEntry>, GscalePortError> {
        let barcode = barcode.trim();
        if barcode.is_empty() {
            return Err(GscalePortError::InvalidInput(
                "barcode is required".to_string(),
            ));
        }
        sqlx::query_as::<_, RawMaterialStockRow>(
            "SELECT id, warehouse, item_code, item_name, barcode, qty, uom,
                    status, reserved_order_id, source_receipt_id
             FROM mini_raw_material_stock
             WHERE lower(barcode) = lower($1)
             ORDER BY updated_at DESC
             LIMIT 1",
        )
        .bind(barcode)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(row_to_stock))
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))
    }

    async fn raw_material_stock(
        &self,
        warehouse: &str,
        limit: usize,
    ) -> Result<Vec<RawMaterialStockEntry>, GscalePortError> {
        let warehouse = warehouse.trim().to_lowercase();
        sqlx::query_as::<_, RawMaterialStockRow>(
            "SELECT id, warehouse, item_code, item_name, barcode, qty, uom,
                    status, reserved_order_id, source_receipt_id
             FROM mini_raw_material_stock
             WHERE $1 = '' OR lower(warehouse) = $1
             ORDER BY lower(warehouse), lower(item_code), updated_at DESC
             LIMIT $2",
        )
        .bind(warehouse)
        .bind(limit.clamp(1, 500) as i64)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(row_to_stock).collect())
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))
    }

    async fn mark_raw_material_stock_in_use(
        &self,
        barcodes: &[String],
        order_id: &str,
    ) -> Result<Vec<RawMaterialStockEntry>, GscalePortError> {
        let barcodes = barcodes
            .iter()
            .map(|barcode| barcode.trim().to_ascii_lowercase())
            .filter(|barcode| !barcode.is_empty())
            .collect::<Vec<_>>();
        let order_id = order_id.trim();
        if barcodes.is_empty() {
            return Ok(Vec::new());
        }
        if order_id.is_empty() {
            return Err(GscalePortError::InvalidInput(
                "order_id is required".to_string(),
            ));
        }
        let rows = sqlx::query_as::<_, RawMaterialStockRow>(
            "UPDATE mini_raw_material_stock
             SET status = 'in_use',
                 reserved_order_id = $2,
                 payload_json = jsonb_set(payload_json, '{in_use_order_id}', to_jsonb($2::text), true),
                 updated_at = now()
             WHERE lower(barcode) = ANY($1)
             RETURNING id, warehouse, item_code, item_name, barcode, qty, uom,
                       status, reserved_order_id, source_receipt_id",
        )
        .bind(&barcodes)
        .bind(order_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        if rows.len() != barcodes.len() {
            return Err(GscalePortError::StoreWrite(
                "raw material stock not found".to_string(),
            ));
        }
        Ok(rows.into_iter().map(row_to_stock).collect())
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

#[derive(sqlx::FromRow)]
struct RawMaterialStockRow {
    id: String,
    warehouse: String,
    item_code: String,
    item_name: String,
    barcode: String,
    qty: f64,
    uom: String,
    status: String,
    reserved_order_id: String,
    source_receipt_id: String,
}

async fn upsert_raw_material_stock(
    pool: &PgPool,
    row: &MaterialReceiptRow,
) -> Result<(), GscalePortError> {
    sqlx::query(
        "INSERT INTO mini_raw_material_stock (
             id, warehouse, item_code, item_name, barcode, qty, uom, status,
             source_receipt_id, payload_json
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'available', $8, $9)
         ON CONFLICT (barcode) DO UPDATE SET
           warehouse = excluded.warehouse,
           item_code = excluded.item_code,
           item_name = excluded.item_name,
           qty = excluded.qty,
           uom = excluded.uom,
           status = excluded.status,
           reserved_order_id = '',
           source_receipt_id = excluded.source_receipt_id,
           payload_json = excluded.payload_json,
           updated_at = now()",
    )
    .bind(raw_stock_id(&row.barcode))
    .bind(row.warehouse.trim())
    .bind(row.item_code.trim())
    .bind(row.item_code.trim())
    .bind(row.barcode.trim())
    .bind(row.qty)
    .bind(row.uom.trim())
    .bind(row.name.trim())
    .bind(serde_json::json!({
        "source_receipt_id": row.name.trim(),
        "source": "mini_gscale_receipts_submit"
    }))
    .execute(pool)
    .await
    .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
    Ok(())
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

fn row_to_stock(row: RawMaterialStockRow) -> RawMaterialStockEntry {
    RawMaterialStockEntry {
        id: row.id,
        warehouse: row.warehouse,
        item_code: row.item_code,
        item_name: row.item_name,
        barcode: row.barcode,
        qty: row.qty,
        uom: row.uom,
        status: row.status,
        reserved_order_id: row.reserved_order_id,
        source_receipt_id: row.source_receipt_id,
    }
}

fn receipt_name(barcode: &str) -> String {
    format!("GSR-{}", barcode.trim().to_ascii_uppercase())
}

fn raw_stock_id(barcode: &str) -> String {
    format!("raw:{}", barcode.trim().to_ascii_lowercase())
}
