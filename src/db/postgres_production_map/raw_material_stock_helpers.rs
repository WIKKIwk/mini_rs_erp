use std::collections::{BTreeMap, BTreeSet};

use sqlx::{Postgres, Transaction};

use crate::core::production_map::{
    ProductionMapError, QueueActionActor, RawMaterialStockTransition, RawMaterialStockTransitionKind,
};
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};

pub(super) async fn apply_raw_material_stock_transitions_tx(
    tx: &mut Transaction<'_, Postgres>,
    transitions: &[RawMaterialStockTransition],
    actor: &QueueActionActor,
    apparatus: &str,
) -> Result<Vec<String>, ProductionMapError> {
    let mut warehouses = BTreeSet::new();
    for transition in transitions {
        if transition.is_empty() {
            continue;
        }
        let barcodes = normalized_barcodes(&transition.barcodes);
        if barcodes.is_empty() || transition.order_id.trim().is_empty() {
            continue;
        }
        let before = raw_material_stock_rows_for_update_tx(tx, &barcodes).await?;
        let owners = raw_material_assignment_owners_tx(tx, &barcodes, &transition.order_id).await?;
        let rows = match transition.kind {
            RawMaterialStockTransitionKind::InUse => {
                mark_raw_material_stock_in_use_tx(tx, &barcodes, &transition.order_id).await
            }
            RawMaterialStockTransitionKind::Consumed => {
                mark_raw_material_stock_consumed_tx(tx, &barcodes, &transition.order_id).await
            }
        }
        .map_err(|error| {
            tracing::error!(
                error = %error,
                order_id = %transition.order_id,
                "failed to update raw material stock inside queue action transaction"
            );
            ProductionMapError::StoreFailed
        })?;
        if rows.len() != barcodes.len() {
            return Err(ProductionMapError::RawMaterialStockUnavailable);
        }
        for row in &rows {
            let previous = before.get(&stock_key(&row.barcode));
            let owner = owners.get(&stock_key(&row.barcode));
            insert_raw_material_event_tx(
                tx,
                stock_transition_event_draft(
                    transition.kind,
                    row,
                    previous.map(|row| row.status.clone()),
                    &transition.order_id,
                    actor,
                    apparatus,
                    owner,
                ),
            )
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        }
        warehouses.extend(
            rows.into_iter()
                .map(|row| row.warehouse.trim().to_string())
                .filter(|warehouse| !warehouse.is_empty()),
        );
    }
    Ok(warehouses.into_iter().collect())
}

async fn mark_raw_material_stock_in_use_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
) -> Result<Vec<RawMaterialStockTransitionRow>, sqlx::Error> {
    sqlx::query_as::<_, RawMaterialStockTransitionRow>(
        "UPDATE mini_raw_material_stock
         SET status = 'in_use',
             reserved_order_id = $2,
             payload_json = jsonb_set(payload_json, '{in_use_order_id}', to_jsonb($2::text), true),
             updated_at = now()
         WHERE lower(barcode) = ANY($1)
           AND (status = 'available' OR (status = 'in_use' AND reserved_order_id = $2))
         RETURNING id, warehouse, item_code, item_name, barcode,
                   qty::float8 AS qty, uom,
                   status, reserved_order_id, source_receipt_id",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .fetch_all(&mut **tx)
    .await
}

async fn mark_raw_material_stock_consumed_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
) -> Result<Vec<RawMaterialStockTransitionRow>, sqlx::Error> {
    sqlx::query_as::<_, RawMaterialStockTransitionRow>(
        "UPDATE mini_raw_material_stock
         SET status = 'consumed',
             payload_json = jsonb_set(payload_json, '{consumed_order_id}', to_jsonb($2::text), true),
             updated_at = now()
         WHERE lower(barcode) = ANY($1)
           AND reserved_order_id = $2
           AND status IN ('in_use', 'consumed')
         RETURNING id, warehouse, item_code, item_name, barcode,
                   qty::float8 AS qty, uom,
                   status, reserved_order_id, source_receipt_id",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .fetch_all(&mut **tx)
    .await
}

#[derive(Clone, sqlx::FromRow)]
struct RawMaterialStockTransitionRow {
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

struct RawMaterialOwner {
    role: String,
    ref_: String,
    display_name: String,
}

async fn raw_material_stock_rows_for_update_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
) -> Result<BTreeMap<String, RawMaterialStockTransitionRow>, ProductionMapError> {
    let rows = sqlx::query_as::<_, RawMaterialStockTransitionRow>(
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
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(rows
        .into_iter()
        .map(|row| (stock_key(&row.barcode), row))
        .collect())
}

async fn raw_material_assignment_owners_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
) -> Result<BTreeMap<String, RawMaterialOwner>, ProductionMapError> {
    let rows = sqlx::query_as::<_, RawMaterialAssignmentOwnerRow>(
        "SELECT barcode,
                COALESCE(payload_json->>'assigned_by_role', '') AS owner_role,
                COALESCE(payload_json->>'assigned_by_ref', '') AS owner_ref,
                COALESCE(payload_json->>'assigned_by_display_name', '') AS owner_display_name
         FROM mini_raw_material_assignments
         WHERE lower(barcode) = ANY($1)
           AND order_id = $2",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                stock_key(&row.barcode),
                RawMaterialOwner {
                    role: row.owner_role,
                    ref_: row.owner_ref,
                    display_name: row.owner_display_name,
                },
            )
        })
        .collect())
}

#[derive(sqlx::FromRow)]
struct RawMaterialAssignmentOwnerRow {
    barcode: String,
    owner_role: String,
    owner_ref: String,
    owner_display_name: String,
}

fn stock_transition_event_draft(
    kind: RawMaterialStockTransitionKind,
    row: &RawMaterialStockTransitionRow,
    previous_status: Option<String>,
    order_id: &str,
    actor: &QueueActionActor,
    apparatus: &str,
    owner: Option<&RawMaterialOwner>,
) -> RawMaterialEventDraft {
    let (event_type, status_after, qty_delta) = match kind {
        RawMaterialStockTransitionKind::InUse => ("usage_started", "in_use", 0.0),
        RawMaterialStockTransitionKind::Consumed => ("consumption_posted", "consumed", -row.qty),
    };
    RawMaterialEventDraft {
        idempotency_key: format!(
            "{}:{}:{}:{}",
            event_type,
            row.barcode.trim().to_ascii_uppercase(),
            order_id.trim(),
            apparatus.trim()
        ),
        event_type: event_type.to_string(),
        warehouse: row.warehouse.trim().to_string(),
        barcode: row.barcode.trim().to_string(),
        item_code: row.item_code.trim().to_string(),
        item_name: row.item_name.trim().to_string(),
        qty_delta,
        uom: row.uom.trim().to_string(),
        stock_status_before: previous_status,
        stock_status_after: Some(status_after.to_string()),
        order_id: Some(order_id.trim().to_string()),
        apparatus: Some(apparatus.trim().to_string()),
        actor_role: actor.role.trim().to_string(),
        actor_ref: actor.ref_.trim().to_string(),
        actor_display_name: actor.display_name.trim().to_string(),
        owner_role: owner
            .filter(|owner| owner.role.trim() == "material_taminotchi")
            .map(|owner| owner.role.trim().to_string())
            .unwrap_or_default(),
        owner_ref: owner
            .filter(|owner| owner.role.trim() == "material_taminotchi")
            .map(|owner| owner.ref_.trim().to_string())
            .unwrap_or_default(),
        owner_display_name: owner
            .filter(|owner| owner.role.trim() == "material_taminotchi")
            .map(|owner| owner.display_name.trim().to_string())
            .unwrap_or_default(),
        source_type: "consumption".to_string(),
        source_id: order_id.trim().to_string(),
        source_line_ref: Some(row.barcode.trim().to_string()),
        correlation_id: None,
        payload_json: serde_json::json!({
            "stock_id": row.id.trim(),
            "barcode": row.barcode.trim(),
            "order_id": order_id.trim(),
            "apparatus": apparatus.trim(),
            "reserved_order_id": row.reserved_order_id.trim(),
            "source_receipt_id": row.source_receipt_id.trim(),
        }),
    }
}

fn stock_key(barcode: &str) -> String {
    barcode.trim().to_ascii_lowercase()
}

fn normalized_barcodes(barcodes: &[String]) -> Vec<String> {
    barcodes
        .iter()
        .map(|barcode| barcode.trim().to_ascii_lowercase())
        .filter(|barcode| !barcode.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
