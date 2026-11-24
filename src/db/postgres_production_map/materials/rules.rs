use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    ProductionMapError, QueueActionActor, RawMaterialAssignment,
};
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};



pub(super) async fn load_raw_material_assignments(
    pool: &PgPool,
) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
    load_raw_material_assignments_scoped(pool, None).await
}

pub(super) async fn load_raw_material_assignments_for_order(
    pool: &PgPool,
    order_id: &str,
) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
    load_raw_material_assignments_scoped(pool, Some(&[order_id.trim().to_string()])).await
}

pub(super) async fn load_transferable_raw_material_assignments(
    pool: &PgPool, order_id: &str, apparatus: &str,
) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
    let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT a.canonical_apparatus_id, a.payload_json FROM mini_raw_material_assignments a
         WHERE a.order_id=$1 AND a.canonical_apparatus_id=$2
           AND NOT EXISTS(SELECT 1 FROM mini_raw_material_stock s
               WHERE lower(s.barcode)=lower(a.barcode) AND s.status='consumed')
         ORDER BY a.barcode",
    ).bind(order_id).bind(apparatus).fetch_all(pool).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(|(id, payload)| raw_material_assignment_from_payload(id, payload)).collect()
}

pub(super) async fn in_use_materials_at_apparatus<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    order_id: &str,
    apparatus: &str,
    lock: bool,
) -> Result<Vec<String>, ProductionMapError> {
    let mut query = String::from(
        "SELECT stock.barcode
         FROM mini_raw_material_stock stock
         JOIN mini_raw_material_assignments assignment
           ON lower(assignment.barcode) = lower(stock.barcode)
         JOIN mini_inventory_placements placement
           ON placement.asset_kind = 'raw_material' AND lower(placement.asset_ref) = lower(stock.id)
         JOIN mini_inventory_locations location
           ON location.id = placement.physical_location_id AND location.kind = 'state' AND location.active
         JOIN mini_factory_location_apparatus_links link
           ON link.location_id = location.factory_location_id
         WHERE stock.status = 'in_use' AND stock.qty > 0 AND stock.reserved_order_id = $1
           AND assignment.order_id = $1 AND assignment.canonical_apparatus_id = $2
           AND link.apparatus_id = $2
         ORDER BY stock.barcode",
    );
    if lock {
        query.push_str(" FOR SHARE OF stock, assignment, placement, location, link");
    }
    let mut barcodes: Vec<String> = sqlx::query_scalar(&query)
        .bind(order_id)
        .bind(apparatus)
        .fetch_all(executor)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    barcodes.dedup();
    Ok(barcodes)
}

pub(super) async fn validate_reused_print_materials_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &crate::core::production_map::OrderRunSession,
) -> Result<(), ProductionMapError> {
    use crate::core::production_map::ProductionQrSessionResources;

    let resources = ProductionQrSessionResources::recorded(session)
        .ok_or(ProductionMapError::RawMaterialScanRequired)?;
    let barcodes = resources.raw_materials.iter()
        .map(|row| row.barcode.clone())
        .collect::<Vec<_>>();
    if barcodes.is_empty() {
        return Err(ProductionMapError::RawMaterialScanRequired);
    }
    let current = sqlx::query_scalar::<_, String>(
        "SELECT session_id FROM mini_order_run_sessions
         WHERE session_id = $1 AND order_id = $2 AND canonical_apparatus_id = $3
           AND status IN ('paused', 'roll_detached')
           AND payload_json->'_print_transfer' = $4
           AND payload_json->'start_resources'->'raw_materials' = $5
           AND payload_json->>'print_transfer_materials_required' = 'true'
         FOR UPDATE",
    )
    .bind(&session.session_id)
    .bind(&session.order_id)
    .bind(&session.apparatus)
    .bind(&session.payload_json["_print_transfer"])
    .bind(serde_json::json!(resources.raw_materials))
    .fetch_optional(&mut **tx).await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if current.is_none() {
        return Err(ProductionMapError::RawMaterialScanRequired);
    }
    let confirmed = in_use_materials_at_apparatus(
        &mut **tx, &session.order_id, &session.apparatus, true,
    ).await?;
    if confirmed.len() != barcodes.len() || !barcodes.iter().all(|barcode| confirmed.iter()
        .any(|current| barcode.trim().eq_ignore_ascii_case(current.trim()))) {
        return Err(ProductionMapError::RawMaterialScanRequired);
    }
    Ok(())
}

pub(super) async fn load_raw_material_assignment_by_barcode(
    pool: &PgPool,
    barcode: &str,
) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
    // The lower(barcode) unique index covers the canonical assignment key.
    let row = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT canonical_apparatus_id, payload_json
         FROM mini_raw_material_assignments
         WHERE lower(barcode) = lower($1)
           AND canonical_apparatus_id IS NOT NULL
           AND EXISTS (
               SELECT 1 FROM mini_apparatus master
               WHERE master.id = mini_raw_material_assignments.canonical_apparatus_id
           )",
    )
    .bind(barcode.trim())
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        tracing::warn!(%error, "raw material assignment lookup failed");
        ProductionMapError::StoreFailed
    })?;
    row.map(|(id, payload)| raw_material_assignment_from_payload(id, payload))
        .transpose()
}

pub(super) async fn load_raw_material_assignments_for_orders(
    pool: &PgPool, order_ids: &[String],
) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
    if order_ids.is_empty() { return Ok(Vec::new()); }
    load_raw_material_assignments_scoped(pool, Some(order_ids)).await
}

async fn load_raw_material_assignments_scoped(
    pool: &PgPool,
    order_ids: Option<&[String]>,
) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
    let mut query = sqlx::QueryBuilder::<Postgres>::new(
        "SELECT canonical_apparatus_id, payload_json
         FROM mini_raw_material_assignments
         WHERE canonical_apparatus_id IS NOT NULL
           AND EXISTS (
               SELECT 1
               FROM mini_apparatus master
               WHERE master.id = mini_raw_material_assignments.canonical_apparatus_id
           )",
    );
    if let Some(order_ids) = order_ids {
        query.push(" AND order_id = ANY(").push_bind(order_ids).push("::TEXT[])");
    }
    query.push(" ORDER BY updated_at DESC");
    let rows = query.build_query_as::<(String, serde_json::Value)>().fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    rows.into_iter()
        .map(|(canonical_apparatus_id, payload)| {
            raw_material_assignment_from_payload(canonical_apparatus_id, payload)
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
    save_raw_material_assignment_tx(&mut tx, &assignment).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn save_raw_material_assignment_tx(
    tx: &mut Transaction<'_, Postgres>,
    assignment: &RawMaterialAssignment,
) -> Result<(), ProductionMapError> {
    let stock = raw_material_stock_for_assignment_tx(tx, &assignment.barcode).await?;
    ensure_assignment_stock_available(&stock.status, &stock.reserved_order_id)?;
    let payload = serde_json::to_value(assignment).map_err(|_| ProductionMapError::StoreFailed)?;
    let result = sqlx::query(
        "INSERT INTO mini_raw_material_assignments
            (barcode, order_id, apparatus, canonical_apparatus_id, item_code, item_group, payload_json, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now())
         ON CONFLICT DO NOTHING",
    )
    .bind(assignment.barcode.trim())
    .bind(assignment.order_id.trim())
    .bind(assignment.apparatus.trim())
    .bind(assignment.apparatus_id.as_str())
    .bind(assignment.item_code.trim())
    .bind(assignment.item_group.trim())
    .bind(payload)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if result.rows_affected() == 0 {
        return Err(ProductionMapError::RawMaterialAlreadyAssigned);
    }
    insert_raw_material_event_tx(tx, assignment_event_draft(assignment, &stock))
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn delete_raw_material_assignment(
    pool: &PgPool,
    order_id: &str,
    barcode: &str,
) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let assignment_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM mini_raw_material_assignments
             WHERE order_id = $1 AND lower(barcode) = lower($2)
         )",
    )
    .bind(order_id.trim())
    .bind(barcode.trim())
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if !assignment_exists {
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        return Ok(None);
    }
    let stock_status = sqlx::query_scalar::<_, String>(
        "SELECT status
         FROM mini_raw_material_stock
         WHERE lower(barcode) = lower($1)
         FOR UPDATE",
    )
    .bind(barcode.trim())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if stock_status
        .as_deref()
        .is_some_and(|status| !status.trim().eq_ignore_ascii_case("available"))
    {
        return Err(ProductionMapError::RawMaterialAssignmentLocked);
    }
    let row = sqlx::query_as::<_, (String, serde_json::Value)>(
        "DELETE FROM mini_raw_material_assignments
         WHERE order_id = $1
           AND lower(barcode) = lower($2)
           AND canonical_apparatus_id IS NOT NULL
           AND EXISTS (
               SELECT 1
               FROM mini_apparatus master
               WHERE master.id = mini_raw_material_assignments.canonical_apparatus_id
           )
         RETURNING canonical_apparatus_id, payload_json",
    )
    .bind(order_id.trim())
    .bind(barcode.trim())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let result = row
        .map(|(canonical_apparatus_id, payload)| {
            raw_material_assignment_from_payload(canonical_apparatus_id, payload)
        })
        .transpose()?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(result)
}

fn raw_material_assignment_from_payload(
    canonical_apparatus_id: String,
    mut payload: serde_json::Value,
) -> Result<RawMaterialAssignment, ProductionMapError> {
    let object = payload
        .as_object_mut()
        .ok_or(ProductionMapError::StoreFailed)?;
    object.insert(
        "apparatus_id".to_string(),
        serde_json::Value::String(canonical_apparatus_id),
    );
    serde_json::from_value::<RawMaterialAssignment>(payload)
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn transfer_raw_material_assignments_tx(
    tx: &mut Transaction<'_, Postgres>,
    assignments: &[RawMaterialAssignment],
    from_apparatus: &str,
    transfer_id: &str,
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    for assignment in assignments {
        let stock = raw_material_stock_for_assignment_tx(tx, &assignment.barcode).await?;
        ensure_assignment_transferable(&stock, &assignment.order_id)?;
        let payload =
            serde_json::to_value(assignment).map_err(|_| ProductionMapError::StoreFailed)?;
        let result = sqlx::query(
            "UPDATE mini_raw_material_assignments
             SET apparatus = $3,
                 canonical_apparatus_id = $4,
                 payload_json = $5,
                 updated_at = now()
             WHERE order_id = $1
               AND lower(barcode) = lower($2)
               AND canonical_apparatus_id = $6",
        )
        .bind(assignment.order_id.trim())
        .bind(assignment.barcode.trim())
        .bind(assignment.apparatus.trim())
        .bind(assignment.apparatus_id.as_str())
        .bind(payload)
        .bind(from_apparatus.trim())
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if result.rows_affected() != 1 {
            return Err(ProductionMapError::RawMaterialAssignmentNotFound);
        }
        insert_raw_material_event_tx(
            tx,
            assignment_transfer_event_draft(
                assignment,
                &stock,
                "order_unreserved",
                from_apparatus,
                transfer_id,
                actor,
            ),
        )
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        insert_raw_material_event_tx(
            tx,
            assignment_transfer_event_draft(
                assignment,
                &stock,
                "order_reserved",
                assignment.apparatus_id.as_str(),
                transfer_id,
                actor,
            ),
        )
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
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
    reserved_order_id: String,
    source_receipt_id: String,
}

async fn raw_material_stock_for_assignment_tx(
    tx: &mut Transaction<'_, Postgres>,
    barcode: &str,
) -> Result<AssignmentStockRow, ProductionMapError> {
    sqlx::query_as::<_, AssignmentStockRow>(
        "SELECT warehouse, item_code, item_name, barcode,
                qty::float8 AS qty, uom, status, reserved_order_id, source_receipt_id
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

fn ensure_assignment_stock_available(
    status: &str,
    reserved_order_id: &str,
) -> Result<(), ProductionMapError> {
    if !status.trim().eq_ignore_ascii_case("available") || !reserved_order_id.trim().is_empty() {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    Ok(())
}

fn ensure_assignment_transferable(
    stock: &AssignmentStockRow,
    order_id: &str,
) -> Result<(), ProductionMapError> {
    if stock.status.trim().eq_ignore_ascii_case("consumed") {
        return Err(ProductionMapError::RawMaterialAssignmentLocked);
    }
    if stock.status.trim().eq_ignore_ascii_case("in_use")
        && stock.reserved_order_id.trim() != order_id.trim()
    {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    Ok(())
}

fn assignment_event_draft(
    assignment: &RawMaterialAssignment,
    stock: &AssignmentStockRow,
) -> RawMaterialEventDraft {
    let actor = QueueActionActor {
        role: assignment.assigned_by_role.clone(),
        ref_: assignment.assigned_by_ref.clone(),
        display_name: assignment.assigned_by_display_name.clone(),
    };
    assignment_event_draft_for(
        assignment,
        stock,
        "order_reserved",
        assignment.apparatus_id.as_str(),
        &actor,
    )
}

include!("rules_parts/assignment_event_draft_for.rs");


#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
