use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::gscale::models::{
    CreateMaterialReceiptDraftInput, MaterialReceiptDraft, RawMaterialStockEntry,
};
use crate::core::gscale::ports::{GscalePortError, MaterialReceiptStorePort};
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};

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
             RETURNING name, item_code, warehouse, qty, uom, barcode, payload_json",
        )
        .bind(name)
        .bind(item_code)
        .bind(warehouse)
        .bind(input.qty)
        .bind(barcode)
        .bind(serde_json::json!({
            "item_code": item_code,
            "item_name": input.item_name.trim(),
            "warehouse": warehouse,
            "qty": input.qty,
            "uom": "kg",
            "barcode": barcode,
            "actor_role": input.actor_role.trim(),
            "actor_ref": input.actor_ref.trim(),
            "actor_display_name": input.actor_display_name.trim(),
        }))
        .fetch_one(&self.pool)
        .await
        .map(row_to_draft)
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))
    }

    async fn submit_stock_entry_draft(&self, name: &str) -> Result<(), GscalePortError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let row = sqlx::query_as::<_, MaterialReceiptRow>(
            "UPDATE mini_gscale_receipts
             SET status = 'submitted', submitted_at = now(), updated_at = now()
             WHERE name = $1 AND status = 'draft'
             RETURNING name, item_code, warehouse, qty, uom, barcode, payload_json",
        )
        .bind(name.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let Some(row) = row else {
            return Err(GscalePortError::StoreWrite(
                "material receipt draft not found".to_string(),
            ));
        };
        let previous_status = raw_material_stock_status_tx(&mut tx, &row.barcode).await?;
        upsert_raw_material_stock_tx(&mut tx, &row).await?;
        insert_raw_material_event_tx(&mut tx, receipt_event_draft(&row, previous_status))
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        tx.commit()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
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
            "SELECT name, item_code, warehouse, qty, uom, barcode, payload_json
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
        let barcodes = normalized_unique_barcodes(barcodes);
        let order_id = order_id.trim();
        if barcodes.is_empty() {
            return Ok(Vec::new());
        }
        if order_id.is_empty() {
            return Err(GscalePortError::InvalidInput(
                "order_id is required".to_string(),
            ));
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let before = raw_material_stock_rows_for_update_tx(&mut tx, &barcodes).await?;
        let rows = sqlx::query_as::<_, RawMaterialStockRow>(
            "UPDATE mini_raw_material_stock
             SET status = 'in_use',
                 reserved_order_id = $2,
                 payload_json = jsonb_set(payload_json, '{in_use_order_id}', to_jsonb($2::text), true),
                 updated_at = now()
             WHERE lower(barcode) = ANY($1)
               AND (status = 'available' OR (status = 'in_use' AND reserved_order_id = $2))
             RETURNING id, warehouse, item_code, item_name, barcode, qty, uom,
                       status, reserved_order_id, source_receipt_id",
        )
        .bind(&barcodes)
        .bind(order_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        if rows.len() != barcodes.len() {
            return Err(GscalePortError::InvalidInput(
                "raw_material_stock_unavailable".to_string(),
            ));
        }
        for row in &rows {
            let previous = before.get(&stock_key(&row.barcode));
            insert_raw_material_event_tx(
                &mut tx,
                stock_status_event_draft(
                    "usage_started",
                    row,
                    previous.map(|row| row.status.clone()),
                    Some("in_use".to_string()),
                    order_id,
                    "system",
                    "system",
                    "",
                ),
            )
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        Ok(rows.into_iter().map(row_to_stock).collect())
    }

    async fn mark_raw_material_stock_consumed(
        &self,
        barcodes: &[String],
        order_id: &str,
    ) -> Result<Vec<RawMaterialStockEntry>, GscalePortError> {
        let barcodes = normalized_unique_barcodes(barcodes);
        let order_id = order_id.trim();
        if barcodes.is_empty() {
            return Ok(Vec::new());
        }
        if order_id.is_empty() {
            return Err(GscalePortError::InvalidInput(
                "order_id is required".to_string(),
            ));
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        let before = raw_material_stock_rows_for_update_tx(&mut tx, &barcodes).await?;
        let rows = sqlx::query_as::<_, RawMaterialStockRow>(
            "UPDATE mini_raw_material_stock
             SET status = 'consumed',
                 payload_json = jsonb_set(payload_json, '{consumed_order_id}', to_jsonb($2::text), true),
                 updated_at = now()
             WHERE lower(barcode) = ANY($1)
               AND reserved_order_id = $2
               AND status IN ('in_use', 'consumed')
             RETURNING id, warehouse, item_code, item_name, barcode, qty, uom,
                       status, reserved_order_id, source_receipt_id",
        )
        .bind(&barcodes)
        .bind(order_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        if rows.len() != barcodes.len() {
            return Err(GscalePortError::InvalidInput(
                "raw_material_stock_unavailable".to_string(),
            ));
        }
        for row in &rows {
            let previous = before.get(&stock_key(&row.barcode));
            insert_raw_material_event_tx(
                &mut tx,
                stock_status_event_draft(
                    "consumption_posted",
                    row,
                    previous.map(|row| row.status.clone()),
                    Some("consumed".to_string()),
                    order_id,
                    "system",
                    "system",
                    "",
                ),
            )
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
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
    payload_json: serde_json::Value,
}

#[derive(Clone, sqlx::FromRow)]
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

async fn raw_material_stock_status_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcode: &str,
) -> Result<Option<String>, GscalePortError> {
    sqlx::query_scalar::<_, String>(
        "SELECT status
         FROM mini_raw_material_stock
         WHERE lower(barcode) = lower($1)
         FOR UPDATE",
    )
    .bind(barcode.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| GscalePortError::StoreWrite(error.to_string()))
}

async fn upsert_raw_material_stock_tx(
    tx: &mut Transaction<'_, Postgres>,
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
    .bind(row_item_name(row))
    .bind(row.barcode.trim())
    .bind(row.qty)
    .bind(row.uom.trim())
    .bind(row.name.trim())
    .bind(serde_json::json!({
        "source_receipt_id": row.name.trim(),
        "source": "mini_gscale_receipts_submit",
        "item_name": row_item_name(row)
    }))
    .execute(&mut **tx)
    .await
    .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
    Ok(())
}

fn receipt_event_draft(
    row: &MaterialReceiptRow,
    previous_status: Option<String>,
) -> RawMaterialEventDraft {
    let actor_role = payload_string(&row.payload_json, "actor_role");
    let actor_ref = payload_string(&row.payload_json, "actor_ref");
    let actor_display_name = payload_string(&row.payload_json, "actor_display_name");
    let owner_is_material = actor_role.trim() == "material_taminotchi";
    RawMaterialEventDraft {
        idempotency_key: format!("receipt_posted:{}", row.name.trim()),
        event_type: "receipt_posted".to_string(),
        warehouse: row.warehouse.trim().to_string(),
        barcode: row.barcode.trim().to_string(),
        item_code: row.item_code.trim().to_string(),
        item_name: row_item_name(row).to_string(),
        qty_delta: row.qty,
        uom: row.uom.trim().to_string(),
        stock_status_before: previous_status,
        stock_status_after: Some("available".to_string()),
        order_id: None,
        apparatus: None,
        actor_role: actor_role.clone(),
        actor_ref: actor_ref.clone(),
        actor_display_name: actor_display_name.clone(),
        owner_role: if owner_is_material {
            actor_role
        } else {
            String::new()
        },
        owner_ref: if owner_is_material {
            actor_ref
        } else {
            String::new()
        },
        owner_display_name: if owner_is_material {
            actor_display_name
        } else {
            String::new()
        },
        source_type: "gscale_receipt".to_string(),
        source_id: row.name.trim().to_string(),
        source_line_ref: Some(row.barcode.trim().to_string()),
        correlation_id: None,
        payload_json: serde_json::json!({
            "receipt_id": row.name.trim(),
            "source_receipt_id": row.name.trim(),
        }),
    }
}

fn row_item_name(row: &MaterialReceiptRow) -> &str {
    let item_name = row
        .payload_json
        .get("item_name")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim();
    if item_name.is_empty() {
        row.item_code.trim()
    } else {
        item_name
    }
}

fn payload_string(payload: &serde_json::Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

async fn raw_material_stock_rows_for_update_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
) -> Result<BTreeMap<String, RawMaterialStockRow>, GscalePortError> {
    let rows = sqlx::query_as::<_, RawMaterialStockRow>(
        "SELECT id, warehouse, item_code, item_name, barcode, qty, uom,
                status, reserved_order_id, source_receipt_id
         FROM mini_raw_material_stock
         WHERE lower(barcode) = ANY($1)
         FOR UPDATE",
    )
    .bind(barcodes)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| GscalePortError::StoreWrite(error.to_string()))?;
    Ok(rows
        .into_iter()
        .map(|row| (stock_key(&row.barcode), row))
        .collect())
}

fn stock_status_event_draft(
    event_type: &str,
    row: &RawMaterialStockRow,
    stock_status_before: Option<String>,
    stock_status_after: Option<String>,
    order_id: &str,
    actor_role: &str,
    actor_ref: &str,
    actor_display_name: &str,
) -> RawMaterialEventDraft {
    let qty_delta = if event_type == "consumption_posted" {
        -row.qty
    } else {
        0.0
    };
    RawMaterialEventDraft {
        idempotency_key: format!(
            "{}:{}:{}",
            event_type,
            row.barcode.trim().to_ascii_uppercase(),
            order_id.trim()
        ),
        event_type: event_type.to_string(),
        warehouse: row.warehouse.trim().to_string(),
        barcode: row.barcode.trim().to_string(),
        item_code: row.item_code.trim().to_string(),
        item_name: row.item_name.trim().to_string(),
        qty_delta,
        uom: row.uom.trim().to_string(),
        stock_status_before,
        stock_status_after,
        order_id: Some(order_id.trim().to_string()),
        apparatus: None,
        actor_role: actor_role.trim().to_string(),
        actor_ref: actor_ref.trim().to_string(),
        actor_display_name: actor_display_name.trim().to_string(),
        owner_role: String::new(),
        owner_ref: String::new(),
        owner_display_name: String::new(),
        source_type: "consumption".to_string(),
        source_id: order_id.trim().to_string(),
        source_line_ref: Some(row.barcode.trim().to_string()),
        correlation_id: None,
        payload_json: serde_json::json!({
            "barcode": row.barcode.trim(),
            "order_id": order_id.trim(),
            "source_receipt_id": row.source_receipt_id.trim(),
        }),
    }
}

fn stock_key(barcode: &str) -> String {
    barcode.trim().to_ascii_lowercase()
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

fn normalized_unique_barcodes(barcodes: &[String]) -> Vec<String> {
    barcodes
        .iter()
        .map(|barcode| barcode.trim().to_ascii_lowercase())
        .filter(|barcode| !barcode.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn receipt_name(barcode: &str) -> String {
    format!("GSR-{}", barcode.trim().to_ascii_uppercase())
}

fn raw_stock_id(barcode: &str) -> String {
    format!("raw:{}", barcode.trim().to_ascii_lowercase())
}
