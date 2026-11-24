//! Premature completion correction, serialized with production, intake and stock writes.
use super::*;
use crate::core::production_map::{
    OrderProgressBatchWipStatus, OrderRunInputStatus, OrderRunStatus, ReactivationPreview,
    ReactivationSelector, ReactivationTarget, chain, order_run_input_links_from_payload,
    write_order_run_input_links,
};
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use std::collections::BTreeSet;
pub(super) mod reclose;

struct Prepared {
    target: ReactivationTarget,
    session: OrderRunSession,
    outputs: Vec<OrderProgressBatch>,
    input: Option<OrderProgressBatch>,
    opening_input: Option<Value>,
    policy: crate::core::production_map::ApparatusQueuePolicy,
    raw: Vec<Value>,
    queue_state: String,
    latest_started: i64,
    latest_session_id: String,
    input_released: bool,
    raw_released: bool,
}
macro_rules! deny {
    ($reason:literal) => {
        return Ok(Err($reason))
    };
}
fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

async fn reactivation_raw_materials(
    tx: &mut Transaction<'_, Postgres>,
    selector: &ReactivationSelector,
    session: &OrderRunSession,
    closing: &Value,
    is_lamination: bool,
    remaining_film_rolls: Option<f64>,
) -> Result<Result<(Vec<Value>, bool), &'static str>, ProductionMapError> {
    let mut raw=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(s) FROM mini_raw_material_stock s JOIN mini_raw_material_assignments a ON lower(a.barcode)=lower(s.barcode) WHERE a.order_id=$1 AND a.canonical_apparatus_id=$2 ORDER BY s.id FOR UPDATE OF s,a")
        .bind(&selector.order_id).bind(&selector.apparatus).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let mut recorded_resources = ProductionQrSessionResources::for_session(session);
    let resource_ids = recorded_resources.raw_materials.iter().filter_map(|material| {
        material.stock_id.clone().or_else(|| raw.iter()
            .find(|row| field(row, "barcode").eq_ignore_ascii_case(&material.barcode))
            .map(|row| field(row, "id").to_string()))
    }).collect::<Vec<_>>();
    let raw_returned = is_lamination && sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM mini_layer_remainder_operations WHERE source_kind='raw_material' AND source_id=ANY($1))")
        .bind(&resource_ids).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let raw_released = is_lamination && (raw_returned
        || (!recorded_resources.raw_materials.is_empty()
            && remaining_film_rolls.is_none_or(|qty| qty <= 0.0))
        || session.payload_json.get("lamination_astatka_raw_released")
            .and_then(Value::as_bool) == Some(true));
    if raw_released {
        // An astatka return already accounted for consumed parent stock and
        // its residual child. Require normal material intake on Resume.
        raw.clear();
        recorded_resources.raw_materials.clear();
    }
    let unlinked = closing
        .get("unused_raw_material_unlinks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if recorded_resources.raw_materials.iter().any(|material| {
        !raw.iter()
            .any(|row| field(row, "barcode").eq_ignore_ascii_case(&material.barcode))
            && !unlinked
                .iter()
                .any(|row| field(row, "barcode").eq_ignore_ascii_case(&material.barcode))
    }) {
        deny!("resource_unavailable");
    }
    if recorded_resources.raw_materials.is_empty() && !raw.is_empty() {
        deny!("unsupported_history");
    }
    raw.retain(|row| {
        recorded_resources
            .raw_materials
            .iter()
            .any(|material| field(row, "barcode").eq_ignore_ascii_case(&material.barcode))
    });
    for stock in &raw {
        let source_material = recorded_resources
            .raw_materials
            .iter()
            .find(|material| field(stock, "barcode").eq_ignore_ascii_case(&material.barcode))
            .ok_or(ProductionMapError::RawMaterialStockUnavailable)?;
        if source_material.item_code != field(stock, "item_code")
            || source_material
                .stock_id
                .as_deref()
                .is_some_and(|id| id != field(stock, "id"))
            || source_material
                .uom
                .as_deref()
                .is_some_and(|uom| uom != field(stock, "uom"))
        {
            deny!("resource_unavailable");
        }
        if !matches!(field(stock, "status"), "consumed" | "in_use")
            || field(stock, "reserved_order_id") != selector.order_id
        {
            deny!("resource_unavailable");
        }
        let source_used: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_layer_remainder_operations WHERE source_kind='raw_material' AND source_id=$1)").bind(field(stock,"id")).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if source_used {
            deny!("resource_unavailable");
        }
        if field(stock, "status") == "consumed" {
            let event: Option<(String,f64,bool)> = sqlx::query_as("SELECT e.event_id,e.qty_delta::float8,(e.payload_json->>'stock_id'=$4 AND ((extract(epoch FROM e.occurred_at)>=$5 AND extract(epoch FROM e.occurred_at)<$6) OR e.payload_json->>'reactivation_reclose_source_session_id'=$7)) FROM mini_raw_material_events e WHERE lower(e.barcode)=lower($1) AND e.order_id=$2 AND e.canonical_apparatus_id=$3 AND e.event_type='consumption_posted' AND NOT EXISTS(SELECT 1 FROM mini_raw_material_events r WHERE r.event_type='consumption_reversed' AND r.payload_json->>'reversed_event_id'=e.event_id) ORDER BY e.id DESC LIMIT 1")
                .bind(field(stock,"barcode")).bind(&selector.order_id).bind(&selector.apparatus).bind(field(stock,"id")).bind(session.started_at_unix as f64).bind((session.updated_at_unix+1) as f64).bind(&session.session_id).fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
            if event.is_none_or(|(_, qty, eligible)| {
                !eligible
                    || stock
                        .get("qty")
                        .and_then(Value::as_f64)
                        .is_none_or(|expected| (qty + expected).abs() > 0.000001)
            }) {
                deny!("resource_unavailable");
            }
        }
    }
    Ok(Ok((raw, raw_released)))
}

include!("reactivation_parts/prepare.rs");

include!("reactivation_parts/preview.rs");
