use std::collections::{BTreeMap, BTreeSet};

use sqlx::{Postgres, Transaction};

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    ProductionMapError, ProductionQrRawMaterial, QueueActionActor, RawMaterialStockTransition,
    RawMaterialStockTransitionKind,
};
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};

#[derive(Debug, Default)]
pub(super) struct RawMaterialStockTransitionOutcome {
    pub(super) warehouses: Vec<String>,
    pub(super) unused_unlinks: Vec<UnusedRawMaterialUnlink>,
    pub(super) started_materials: Vec<ProductionQrRawMaterial>,
}

#[derive(Debug, Clone)]
pub(super) struct UnusedRawMaterialUnlink {
    pub(super) barcode: String,
    pub(super) stock_status: String,
    pub(super) reserved_order_id: String,
    pub(super) warehouse: String,
}

pub(super) async fn apply_raw_material_stock_transitions_tx(
    tx: &mut Transaction<'_, Postgres>,
    transitions: &[RawMaterialStockTransition],
    actor: &QueueActionActor,
    apparatus: &str,
) -> Result<RawMaterialStockTransitionOutcome, ProductionMapError> {
    apply_raw_material_stock_transitions_for_event_tx(tx, transitions, actor, apparatus, None).await
}

pub(super) async fn apply_raw_material_stock_transitions_for_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    transitions: &[RawMaterialStockTransition],
    actor: &QueueActionActor,
    apparatus: &str,
    queue_event: Option<&str>,
) -> Result<RawMaterialStockTransitionOutcome, ProductionMapError> {
    let apparatus_id = ApparatusId::new(apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::RawMaterialInvalidInput)?;
    let mut warehouses = BTreeSet::new();
    let mut unused_unlinks = Vec::new();
    let mut started_materials = Vec::new();
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
                let rows = mark_raw_material_stock_in_use_tx(
                    tx,
                    &barcodes,
                    &transition.order_id,
                    apparatus_id.as_str(),
                )
                .await
                .map_err(stock_transition_store_error(&transition.order_id))?;
                ensure_stock_transition_rows_affected(transition.kind, barcodes.len(), rows.len())?;
                rows
            }
            RawMaterialStockTransitionKind::Consumed => {
                let rows = mark_raw_material_stock_consumed_tx(
                    tx,
                    &barcodes,
                    &transition.order_id,
                    apparatus_id.as_str(),
                )
                .await
                .map_err(stock_transition_store_error(&transition.order_id))?;
                ensure_stock_transition_rows_affected(transition.kind, barcodes.len(), rows.len())?;
                rows
            }
            RawMaterialStockTransitionKind::Complete => {
                let settlement = settle_completion_raw_materials_tx(
                    tx,
                    &barcodes,
                    &transition.order_id,
                    apparatus_id.as_str(),
                    &before,
                )
                .await?;
                warehouses.extend(
                    settlement
                        .unused_unlinks
                        .iter()
                        .map(|unlink| unlink.warehouse.trim().to_string())
                        .filter(|warehouse| !warehouse.is_empty()),
                );
                unused_unlinks.extend(settlement.unused_unlinks);
                settlement.consumed_rows
            }
        };
        if transition.kind == RawMaterialStockTransitionKind::InUse {
            // Only a fresh physical intake changes the roll binding. Resume or
            // a second shift observing older in-use stock must not replace it.
            let activated_ids = rows
                .iter()
                .filter(|row| {
                    before
                        .get(&stock_key(&row.barcode))
                        .is_some_and(|old| old.status != "in_use")
                })
                .map(|row| row.id.clone())
                .collect::<Vec<_>>();
            bind_lamination_input_roll_tx(
                tx,
                &activated_ids,
                &transition.order_id,
                apparatus_id.as_str(),
            )
            .await?;
        }
        for row in &rows {
            if transition.kind == RawMaterialStockTransitionKind::InUse {
                started_materials.push(row.start_material_snapshot());
            }
            let previous = before.get(&stock_key(&row.barcode));
            let owner = owners.get(&stock_key(&row.barcode));
            let mut draft = stock_transition_event_draft(
                transition.kind,
                row,
                previous.map(|row| row.status.clone()),
                &transition.order_id,
                actor,
                apparatus_id.as_str(),
                owner,
            );
            if transition.kind == RawMaterialStockTransitionKind::InUse
                && let Some(event) = queue_event
            {
                // A cancelled Start can later claim the same material again.
                // Its fresh usage needs its own durable audit receipt.
                draft.idempotency_key = format!("{}:{event}", draft.idempotency_key);
                draft.correlation_id = Some(event.into());
            }
            insert_raw_material_event_tx(tx, draft)
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
        }
        warehouses.extend(
            rows.into_iter()
                .map(|row| row.warehouse.trim().to_string())
                .filter(|warehouse| !warehouse.is_empty()),
        );
    }
    Ok(RawMaterialStockTransitionOutcome {
        warehouses: warehouses.into_iter().collect(),
        unused_unlinks,
        started_materials,
    })
}

async fn bind_lamination_input_roll_tx(
    tx: &mut Transaction<'_, Postgres>,
    activated_ids: &[String],
    order_id: &str,
    apparatus_id: &str,
) -> Result<(), ProductionMapError> {
    if activated_ids.is_empty() {
        return Ok(());
    }
    let rolls: Vec<String> = sqlx::query_scalar(
        "SELECT s.id FROM mini_raw_material_stock s
         JOIN mini_apparatus a ON a.id=$2
         WHERE s.id=ANY($1) AND s.uom='kg' AND s.length_m>0
           AND a.execution_profile_json->>'operation'='laminate'",
    )
    .bind(activated_ids)
    .bind(apparatus_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    // A bulk claim containing several rolls has no physical ordering. Never
    // pick by barcode or SQL result order; sequential scans each bind one roll.
    if rolls.len() > 1 {
        sqlx::query("UPDATE mini_raw_material_assignments SET payload_json=payload_json-'lamination_input_stock_id' WHERE order_id=$1 AND canonical_apparatus_id=$2")
            .bind(order_id).bind(apparatus_id).execute(&mut **tx)
            .await.map_err(|_| ProductionMapError::StoreFailed)?;
    } else if let [stock_id] = rolls.as_slice() {
        sqlx::query(
            "UPDATE mini_raw_material_assignments r
             SET payload_json=CASE WHEN lower(r.barcode)=lower(s.barcode)
                 THEN (r.payload_json-'lamination_input_stock_id')
                      || jsonb_build_object('lamination_input_stock_id',s.id)
                 ELSE r.payload_json-'lamination_input_stock_id' END
             FROM mini_raw_material_stock s
             WHERE s.id=$1 AND r.order_id=$2 AND r.canonical_apparatus_id=$3",
        )
        .bind(stock_id)
        .bind(order_id)
        .bind(apparatus_id)
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}

fn stock_transition_store_error(
    order_id: &str,
) -> impl FnOnce(sqlx::Error) -> ProductionMapError + '_ {
    move |error| {
        tracing::error!(
            error = %error,
            order_id,
            "failed to update raw material stock inside queue action transaction"
        );
        ProductionMapError::StoreFailed
    }
}

struct CompletionMaterialSettlement {
    consumed_rows: Vec<RawMaterialStockTransitionRow>,
    unused_unlinks: Vec<UnusedRawMaterialUnlink>,
}

async fn settle_completion_raw_materials_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
    apparatus_id: &str,
    before: &BTreeMap<String, RawMaterialStockTransitionRow>,
) -> Result<CompletionMaterialSettlement, ProductionMapError> {
    if before.len() != barcodes.len() {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    let order_id = order_id.trim();
    let mut consumed_barcodes = Vec::new();
    let mut unused_barcodes = Vec::new();
    for barcode in barcodes {
        let row = before
            .get(&stock_key(barcode))
            .ok_or(ProductionMapError::RawMaterialStockUnavailable)?;
        let status = row.status.trim().to_ascii_lowercase();
        let reservation = row.reserved_order_id.trim();
        if matches!(status.as_str(), "in_use" | "consumed") && reservation == order_id {
            consumed_barcodes.push(barcode.clone());
        } else if status == "available" && (reservation.is_empty() || reservation == order_id) {
            unused_barcodes.push(barcode.clone());
        } else {
            return Err(ProductionMapError::RawMaterialStockUnavailable);
        }
    }

    let consumed_rows = if consumed_barcodes.is_empty() {
        Vec::new()
    } else {
        let rows =
            mark_raw_material_stock_consumed_tx(tx, &consumed_barcodes, order_id, apparatus_id)
                .await
                .map_err(stock_transition_store_error(order_id))?;
        ensure_stock_transition_rows_affected(
            RawMaterialStockTransitionKind::Consumed,
            consumed_barcodes.len(),
            rows.len(),
        )?;
        rows
    };

    let unused_unlinks = if unused_barcodes.is_empty() {
        Vec::new()
    } else {
        unlink_unused_raw_material_assignments_tx(tx, &unused_barcodes, order_id, apparatus_id)
            .await?;
        unused_barcodes
            .iter()
            .filter_map(|barcode| before.get(&stock_key(barcode)))
            .map(|row| UnusedRawMaterialUnlink {
                barcode: row.barcode.trim().to_string(),
                stock_status: row.status.trim().to_string(),
                reserved_order_id: row.reserved_order_id.trim().to_string(),
                warehouse: row.warehouse.trim().to_string(),
            })
            .collect()
    };

    Ok(CompletionMaterialSettlement {
        consumed_rows,
        unused_unlinks,
    })
}

async fn unlink_unused_raw_material_assignments_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
    apparatus_id: &str,
) -> Result<(), ProductionMapError> {
    let removed = sqlx::query_scalar::<_, String>(
        "DELETE FROM mini_raw_material_assignments
         WHERE lower(barcode) = ANY($1)
           AND order_id = $2
           AND canonical_apparatus_id = $3
         RETURNING barcode",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .bind(apparatus_id.trim())
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if removed.len() != barcodes.len() {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    sqlx::query(
        "UPDATE mini_raw_material_stock
         SET reserved_order_id = '',
             payload_json = payload_json - 'in_use_order_id',
             updated_at = now()
         WHERE lower(barcode) = ANY($1)
           AND status = 'available'
           AND reserved_order_id = $2",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

async fn mark_raw_material_stock_in_use_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
    apparatus_id: &str,
) -> Result<Vec<RawMaterialStockTransitionRow>, sqlx::Error> {
    sqlx::query_as::<_, RawMaterialStockTransitionRow>(
        "UPDATE mini_raw_material_stock AS stock
         SET status = 'in_use',
             reserved_order_id = $2,
             payload_json = jsonb_set(stock.payload_json, '{in_use_order_id}', to_jsonb($2::text), true),
             updated_at = now()
         WHERE lower(stock.barcode) = ANY($1)
           AND (stock.status = 'available' OR (stock.status = 'in_use' AND stock.reserved_order_id = $2))
           AND EXISTS (
               SELECT 1
               FROM mini_raw_material_assignments AS assignment
               WHERE lower(assignment.barcode) = lower(stock.barcode)
                 AND assignment.order_id = $2
                 AND assignment.canonical_apparatus_id = $3
           )
         RETURNING id, warehouse, item_code, item_name, barcode,
                   qty::float8 AS qty, uom,
                   status, reserved_order_id, source_receipt_id",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .bind(apparatus_id.trim())
    .fetch_all(&mut **tx)
    .await
}

async fn mark_raw_material_stock_consumed_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcodes: &[String],
    order_id: &str,
    apparatus_id: &str,
) -> Result<Vec<RawMaterialStockTransitionRow>, sqlx::Error> {
    sqlx::query_as::<_, RawMaterialStockTransitionRow>(
        "UPDATE mini_raw_material_stock AS stock
         SET status = 'consumed',
             payload_json = jsonb_set(stock.payload_json, '{consumed_order_id}', to_jsonb($2::text), true),
             updated_at = now()
         WHERE lower(stock.barcode) = ANY($1)
           AND stock.reserved_order_id = $2
           AND stock.status IN ('in_use', 'consumed')
           AND EXISTS (
               SELECT 1
               FROM mini_raw_material_assignments AS assignment
               WHERE lower(assignment.barcode) = lower(stock.barcode)
                 AND assignment.order_id = $2
                 AND assignment.canonical_apparatus_id = $3
           )
         RETURNING id, warehouse, item_code, item_name, barcode,
                   qty::float8 AS qty, uom,
                   status, reserved_order_id, source_receipt_id",
    )
    .bind(barcodes)
    .bind(order_id.trim())
    .bind(apparatus_id.trim())
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

impl RawMaterialStockTransitionRow {
    fn start_material_snapshot(&self) -> ProductionQrRawMaterial {
        ProductionQrRawMaterial {
            barcode: self.barcode.clone(),
            item_code: self.item_code.clone(),
            item_name: self.item_name.clone(),
            stock_id: Some(self.id.clone()),
            source_qty: Some(self.qty),
            uom: Some(self.uom.clone()),
        }
    }
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

include!("stock_parts/raw_material_assignment_owners_tx.rs");

#[cfg(test)]
#[path = "stock_tests.rs"]
mod tests;
