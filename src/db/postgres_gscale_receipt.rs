use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::gscale::models::{
    CreateMaterialReceiptDraftInput, MaterialReceiptDraft, RawMaterialStockEntry,
    RawMaterialStockUpdateInput,
};
use crate::core::gscale::ports::{GscalePortError, MaterialReceiptStorePort};
use crate::core::quantity::positive_erp_quantity;
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

include!("postgres_gscale_receipt_store.rs");

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
         VALUES ($1, $2, $3, $4, $5,
                 ($6::double precision)::numeric(24,9), $7, 'available', $8, $9)
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
        "SELECT id, warehouse, item_code, item_name, barcode,
                qty::float8 AS qty, uom,
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
    actor: (&str, &str, &str),
) -> RawMaterialEventDraft {
    let (actor_role, actor_ref, actor_display_name) = actor;
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

fn raw_material_stock_correction_event(
    previous: &RawMaterialStockRow,
    updated: &RawMaterialStockRow,
    input: &RawMaterialStockUpdateInput,
) -> RawMaterialEventDraft {
    let random: [u8; 16] = rand::random();
    let owner_is_material = input.actor_role.trim() == "material_taminotchi";
    RawMaterialEventDraft {
        idempotency_key: format!(
            "stock_corrected:{}",
            data_encoding::HEXLOWER.encode(&random)
        ),
        event_type: "stock_corrected".to_string(),
        warehouse: updated.warehouse.trim().to_string(),
        barcode: updated.barcode.trim().to_string(),
        item_code: updated.item_code.trim().to_string(),
        item_name: updated.item_name.trim().to_string(),
        qty_delta: updated.qty - previous.qty,
        uom: updated.uom.trim().to_string(),
        stock_status_before: Some(previous.status.trim().to_string()),
        stock_status_after: Some(updated.status.trim().to_string()),
        order_id: None,
        apparatus: None,
        actor_role: input.actor_role.trim().to_string(),
        actor_ref: input.actor_ref.trim().to_string(),
        actor_display_name: input.actor_display_name.trim().to_string(),
        owner_role: if owner_is_material {
            input.actor_role.trim().to_string()
        } else {
            String::new()
        },
        owner_ref: if owner_is_material {
            input.actor_ref.trim().to_string()
        } else {
            String::new()
        },
        owner_display_name: if owner_is_material {
            input.actor_display_name.trim().to_string()
        } else {
            String::new()
        },
        source_type: "stock_correction".to_string(),
        source_id: updated.source_receipt_id.trim().to_string(),
        source_line_ref: Some(updated.barcode.trim().to_string()),
        correlation_id: None,
        payload_json: serde_json::json!({
            "stock_id": updated.id.trim(),
            "source_receipt_id": updated.source_receipt_id.trim(),
            "previous_item_code": previous.item_code.trim(),
            "previous_item_name": previous.item_name.trim(),
            "previous_qty": previous.qty,
            "item_code": updated.item_code.trim(),
            "item_name": updated.item_name.trim(),
            "qty": updated.qty,
            "barcode_unchanged": true,
            "source_receipt_id_unchanged": true,
        }),
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
