use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    ApparatusMaterialRule, ProductionMapError, RawMaterialAssignment,
};
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};

pub(super) async fn load_apparatus_material_rules(
    pool: &PgPool,
) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
    let rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json
         FROM mini_apparatus_material_rules
         ORDER BY lower(apparatus) ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    rows.into_iter()
        .map(|payload| {
            serde_json::from_value::<ApparatusMaterialRule>(payload)
                .map_err(|_| ProductionMapError::StoreFailed)
        })
        .collect()
}

pub(super) async fn save_apparatus_material_rule(
    pool: &PgPool,
    rule: ApparatusMaterialRule,
) -> Result<(), ProductionMapError> {
    let item_groups =
        serde_json::to_value(&rule.item_groups).map_err(|_| ProductionMapError::StoreFailed)?;
    let requirement_groups = serde_json::to_value(&rule.requirement_groups)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let payload = serde_json::to_value(&rule).map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_apparatus_material_rules
            (apparatus, item_groups, requirement_groups, requires_material, payload_json, updated_at)
         VALUES ($1, $2, $3, $4, $5, now())
         ON CONFLICT (apparatus) DO UPDATE SET
           item_groups = excluded.item_groups,
           requirement_groups = excluded.requirement_groups,
           requires_material = excluded.requires_material,
           payload_json = excluded.payload_json,
           updated_at = excluded.updated_at",
    )
    .bind(rule.apparatus.trim())
    .bind(item_groups)
    .bind(requirement_groups)
    .bind(rule.requires_material)
    .bind(payload)
    .execute(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn load_raw_material_assignments(
    pool: &PgPool,
) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
    let rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json
         FROM mini_raw_material_assignments
         ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    rows.into_iter()
        .map(|payload| {
            serde_json::from_value::<RawMaterialAssignment>(payload)
                .map_err(|_| ProductionMapError::StoreFailed)
        })
        .collect()
}

pub(super) async fn save_raw_material_assignment(
    pool: &PgPool,
    assignment: RawMaterialAssignment,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let stock = raw_material_stock_for_assignment_tx(&mut tx, &assignment.barcode).await?;
    let payload = serde_json::to_value(&assignment).map_err(|_| ProductionMapError::StoreFailed)?;
    let result = sqlx::query(
        "INSERT INTO mini_raw_material_assignments
            (barcode, order_id, apparatus, item_code, item_group, payload_json, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, now())
         ON CONFLICT (barcode) DO NOTHING",
    )
    .bind(assignment.barcode.trim())
    .bind(assignment.order_id.trim())
    .bind(assignment.apparatus.trim())
    .bind(assignment.item_code.trim())
    .bind(assignment.item_group.trim())
    .bind(payload)
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if result.rows_affected() == 0 {
        return Err(ProductionMapError::RawMaterialAlreadyAssigned);
    }
    insert_raw_material_event_tx(&mut tx, assignment_event_draft(&assignment, &stock))
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn delete_raw_material_assignment(
    pool: &PgPool,
    order_id: &str,
    barcode: &str,
) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
    let row = sqlx::query_scalar::<_, serde_json::Value>(
        "DELETE FROM mini_raw_material_assignments
         WHERE order_id = $1
           AND lower(barcode) = lower($2)
         RETURNING payload_json",
    )
    .bind(order_id.trim())
    .bind(barcode.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    row.map(|payload| {
        serde_json::from_value::<RawMaterialAssignment>(payload)
            .map_err(|_| ProductionMapError::StoreFailed)
    })
    .transpose()
}

#[derive(sqlx::FromRow)]
struct AssignmentStockRow {
    warehouse: String,
    item_code: String,
    item_name: String,
    barcode: String,
    qty: f64,
    uom: String,
    status: String,
    source_receipt_id: String,
}

async fn raw_material_stock_for_assignment_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcode: &str,
) -> Result<AssignmentStockRow, ProductionMapError> {
    sqlx::query_as::<_, AssignmentStockRow>(
        "SELECT warehouse, item_code, item_name, barcode, qty, uom, status, source_receipt_id
         FROM mini_raw_material_stock
         WHERE lower(barcode) = lower($1)
         FOR UPDATE",
    )
    .bind(barcode.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::RawMaterialStockUnavailable)
}

fn assignment_event_draft(
    assignment: &RawMaterialAssignment,
    stock: &AssignmentStockRow,
) -> RawMaterialEventDraft {
    RawMaterialEventDraft {
        idempotency_key: format!(
            "order_reserved:{}:{}:{}",
            assignment.barcode.trim().to_ascii_uppercase(),
            assignment.order_id.trim(),
            assignment.apparatus.trim()
        ),
        event_type: "order_reserved".to_string(),
        warehouse: stock.warehouse.trim().to_string(),
        barcode: stock.barcode.trim().to_string(),
        item_code: stock.item_code.trim().to_string(),
        item_name: blank_default(&assignment.item_name, &stock.item_name).to_string(),
        qty_delta: 0.0,
        uom: stock.uom.trim().to_string(),
        stock_status_before: Some(stock.status.trim().to_string()),
        stock_status_after: Some(stock.status.trim().to_string()),
        order_id: Some(assignment.order_id.trim().to_string()),
        apparatus: Some(assignment.apparatus.trim().to_string()),
        actor_role: assignment.assigned_by_role.trim().to_string(),
        actor_ref: assignment.assigned_by_ref.trim().to_string(),
        actor_display_name: assignment.assigned_by_display_name.trim().to_string(),
        owner_role: if assignment.assigned_by_role.trim() == "material_taminotchi" {
            "material_taminotchi".to_string()
        } else {
            String::new()
        },
        owner_ref: if assignment.assigned_by_role.trim() == "material_taminotchi" {
            assignment.assigned_by_ref.trim().to_string()
        } else {
            String::new()
        },
        owner_display_name: if assignment.assigned_by_role.trim() == "material_taminotchi" {
            assignment.assigned_by_display_name.trim().to_string()
        } else {
            String::new()
        },
        source_type: "order_assignment".to_string(),
        source_id: assignment.order_id.trim().to_string(),
        source_line_ref: Some(assignment.barcode.trim().to_string()),
        correlation_id: None,
        payload_json: serde_json::json!({
            "order_id": assignment.order_id.trim(),
            "apparatus": assignment.apparatus.trim(),
            "barcode": assignment.barcode.trim(),
            "item_group": assignment.item_group.trim(),
            "source_receipt_id": stock.source_receipt_id.trim(),
            "qty": stock.qty,
        }),
    }
}

fn blank_default<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim()
    } else {
        value
    }
}
