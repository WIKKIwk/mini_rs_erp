use std::collections::BTreeSet;

use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};

#[derive(Debug, Clone)]
pub struct RawMaterialEventDraft {
    pub idempotency_key: String,
    pub event_type: String,
    pub warehouse: String,
    pub barcode: String,
    pub item_code: String,
    pub item_name: String,
    pub qty_delta: f64,
    pub uom: String,
    pub stock_status_before: Option<String>,
    pub stock_status_after: Option<String>,
    pub order_id: Option<String>,
    pub apparatus: Option<String>,
    pub actor_role: String,
    pub actor_ref: String,
    pub actor_display_name: String,
    pub owner_role: String,
    pub owner_ref: String,
    pub owner_display_name: String,
    pub source_type: String,
    pub source_id: String,
    pub source_line_ref: Option<String>,
    pub correlation_id: Option<String>,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawMaterialEventEntry {
    pub id: i64,
    pub event_id: String,
    pub event_type: String,
    pub warehouse: String,
    pub barcode: String,
    pub item_code: String,
    pub item_name: String,
    pub qty_delta: f64,
    pub uom: String,
    pub stock_status_before: String,
    pub stock_status_after: String,
    pub order_id: String,
    pub apparatus: String,
    pub actor_role: String,
    pub actor_ref: String,
    pub actor_display_name: String,
    pub owner_role: String,
    pub owner_ref: String,
    pub owner_display_name: String,
    pub source_type: String,
    pub source_id: String,
    pub source_line_ref: String,
    pub correlation_id: String,
    pub payload_json: serde_json::Value,
    pub occurred_at_unix: i64,
    pub recorded_at_unix: i64,
}

#[derive(Clone)]
pub struct PostgresRawMaterialEventStore {
    pool: PgPool,
}

impl PostgresRawMaterialEventStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn record_event(&self, draft: RawMaterialEventDraft) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        insert_raw_material_event_tx(&mut tx, draft).await?;
        tx.commit().await
    }

    pub async fn events(
        &self,
        scope: RawMaterialEventScope,
        query: RawMaterialEventQuery,
    ) -> Result<Vec<RawMaterialEventEntry>, sqlx::Error> {
        let scoped_warehouses = scope
            .warehouses
            .into_iter()
            .map(|warehouse| warehouse.trim().to_lowercase())
            .filter(|warehouse| !warehouse.is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if scope.enabled && scoped_warehouses.is_empty() {
            return Ok(Vec::new());
        }
        let warehouse = query.warehouse.trim();
        let event_type = query.event_type.trim();
        let actor_role = query.actor_role.trim();
        let actor_ref = query.actor_ref.trim();
        let owner_role = query.owner_role.trim();
        let owner_ref = query.owner_ref.trim();
        let limit = query.limit.clamp(1, 200) as i64;
        sqlx::query_as::<_, RawMaterialEventRow>(
            "SELECT id, event_id, event_type, warehouse, barcode, item_code, item_name,
                    qty_delta::double precision AS qty_delta, uom,
                    COALESCE(stock_status_before, '') AS stock_status_before,
                    COALESCE(stock_status_after, '') AS stock_status_after,
                    COALESCE(order_id, '') AS order_id,
                    COALESCE(apparatus, '') AS apparatus,
                    actor_role, actor_ref, actor_display_name,
                    owner_role, owner_ref, owner_display_name,
                    source_type, source_id,
                    COALESCE(source_line_ref, '') AS source_line_ref,
                    COALESCE(correlation_id, '') AS correlation_id,
                    payload_json,
                    EXTRACT(EPOCH FROM occurred_at)::bigint AS occurred_at_unix,
                    EXTRACT(EPOCH FROM recorded_at)::bigint AS recorded_at_unix
             FROM mini_raw_material_events
             WHERE ($1 = false OR warehouse_key = ANY($2))
               AND ($3 = '' OR warehouse_key = lower($3))
               AND ($4 = '' OR event_type = $4)
               AND ($5 = '' OR actor_role = $5)
               AND ($6 = '' OR actor_ref = $6)
               AND ($7 = '' OR owner_role = $7)
               AND ($8 = '' OR owner_ref = $8)
             ORDER BY occurred_at DESC, id DESC
             LIMIT $9",
        )
        .bind(scope.enabled)
        .bind(scoped_warehouses)
        .bind(warehouse)
        .bind(event_type)
        .bind(actor_role)
        .bind(actor_ref)
        .bind(owner_role)
        .bind(owner_ref)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(row_to_entry).collect())
    }
}

#[derive(Debug, Clone, Default)]
pub struct RawMaterialEventScope {
    pub enabled: bool,
    pub warehouses: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RawMaterialEventQuery {
    pub warehouse: String,
    pub event_type: String,
    pub actor_role: String,
    pub actor_ref: String,
    pub owner_role: String,
    pub owner_ref: String,
    pub limit: usize,
}

#[derive(sqlx::FromRow)]
struct RawMaterialEventRow {
    id: i64,
    event_id: String,
    event_type: String,
    warehouse: String,
    barcode: String,
    item_code: String,
    item_name: String,
    qty_delta: f64,
    uom: String,
    stock_status_before: String,
    stock_status_after: String,
    order_id: String,
    apparatus: String,
    actor_role: String,
    actor_ref: String,
    actor_display_name: String,
    owner_role: String,
    owner_ref: String,
    owner_display_name: String,
    source_type: String,
    source_id: String,
    source_line_ref: String,
    correlation_id: String,
    payload_json: serde_json::Value,
    occurred_at_unix: i64,
    recorded_at_unix: i64,
}

pub async fn insert_raw_material_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    draft: RawMaterialEventDraft,
) -> Result<Option<String>, sqlx::Error> {
    let draft = normalize_draft(draft);
    sqlx::query_scalar::<_, String>(
        "INSERT INTO mini_raw_material_events (
             event_id, idempotency_key, event_type, warehouse, barcode, item_code, item_name,
             qty_delta, uom, stock_status_before, stock_status_after, order_id, apparatus,
             actor_role, actor_ref, actor_display_name,
             owner_role, owner_ref, owner_display_name,
             source_type, source_id,
             source_line_ref, correlation_id, payload_json
         )
         VALUES (
             $1, $2, $3, $4, $5, $6, $7, ($8::double precision)::numeric(18,3), $9,
             $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24
         )
         ON CONFLICT (idempotency_key) DO NOTHING
         RETURNING event_id",
    )
    .bind(new_event_id())
    .bind(draft.idempotency_key)
    .bind(draft.event_type)
    .bind(draft.warehouse)
    .bind(draft.barcode)
    .bind(draft.item_code)
    .bind(draft.item_name)
    .bind(draft.qty_delta)
    .bind(draft.uom)
    .bind(draft.stock_status_before)
    .bind(draft.stock_status_after)
    .bind(draft.order_id)
    .bind(draft.apparatus)
    .bind(draft.actor_role)
    .bind(draft.actor_ref)
    .bind(draft.actor_display_name)
    .bind(draft.owner_role)
    .bind(draft.owner_ref)
    .bind(draft.owner_display_name)
    .bind(draft.source_type)
    .bind(draft.source_id)
    .bind(draft.source_line_ref)
    .bind(draft.correlation_id)
    .bind(draft.payload_json)
    .fetch_optional(&mut **tx)
    .await
}

fn normalize_draft(mut draft: RawMaterialEventDraft) -> RawMaterialEventDraft {
    draft.idempotency_key = blank_default(&draft.idempotency_key, "raw_material_event").to_string();
    draft.event_type = draft.event_type.trim().to_string();
    draft.warehouse = draft.warehouse.trim().to_string();
    draft.barcode = draft.barcode.trim().to_ascii_uppercase();
    draft.item_code = draft.item_code.trim().to_string();
    draft.item_name = blank_default(&draft.item_name, &draft.item_code).to_string();
    draft.uom = blank_default(&draft.uom, "kg").to_string();
    draft.stock_status_before = clean_optional(draft.stock_status_before);
    draft.stock_status_after = clean_optional(draft.stock_status_after);
    draft.order_id = clean_optional(draft.order_id);
    draft.apparatus = clean_optional(draft.apparatus);
    draft.actor_role = blank_default(&draft.actor_role, "system").to_string();
    draft.actor_ref = blank_default(&draft.actor_ref, "system").to_string();
    draft.actor_display_name = draft.actor_display_name.trim().to_string();
    draft.owner_role = draft.owner_role.trim().to_string();
    draft.owner_ref = draft.owner_ref.trim().to_string();
    draft.owner_display_name = draft.owner_display_name.trim().to_string();
    draft.source_type = blank_default(&draft.source_type, "system").to_string();
    draft.source_id = blank_default(&draft.source_id, &draft.idempotency_key).to_string();
    draft.source_line_ref = clean_optional(draft.source_line_ref);
    draft.correlation_id = clean_optional(draft.correlation_id);
    draft
}

fn row_to_entry(row: RawMaterialEventRow) -> RawMaterialEventEntry {
    RawMaterialEventEntry {
        id: row.id,
        event_id: row.event_id,
        event_type: row.event_type,
        warehouse: row.warehouse,
        barcode: row.barcode,
        item_code: row.item_code,
        item_name: row.item_name,
        qty_delta: row.qty_delta,
        uom: row.uom,
        stock_status_before: row.stock_status_before,
        stock_status_after: row.stock_status_after,
        order_id: row.order_id,
        apparatus: row.apparatus,
        actor_role: row.actor_role,
        actor_ref: row.actor_ref,
        actor_display_name: row.actor_display_name,
        owner_role: row.owner_role,
        owner_ref: row.owner_ref,
        owner_display_name: row.owner_display_name,
        source_type: row.source_type,
        source_id: row.source_id,
        source_line_ref: row.source_line_ref,
        correlation_id: row.correlation_id,
        payload_json: row.payload_json,
        occurred_at_unix: row.occurred_at_unix,
        recorded_at_unix: row.recorded_at_unix,
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn blank_default<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim()
    } else {
        value
    }
}

fn new_event_id() -> String {
    let bytes: [u8; 16] = rand::random();
    format!("rme_{}", data_encoding::HEXLOWER.encode(&bytes))
}
