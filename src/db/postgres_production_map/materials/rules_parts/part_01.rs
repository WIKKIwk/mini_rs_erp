

pub(super) async fn load_raw_material_assignments(
    pool: &PgPool,
) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
    let rows = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT canonical_apparatus_id, payload_json
         FROM mini_raw_material_assignments
         WHERE canonical_apparatus_id IS NOT NULL
           AND EXISTS (
               SELECT 1
               FROM mini_apparatus master
               WHERE master.id = mini_raw_material_assignments.canonical_apparatus_id
           )
         ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
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

fn assignment_event_draft_for(
    assignment: &RawMaterialAssignment,
    stock: &AssignmentStockRow,
    event_type: &str,
    apparatus_id: &str,
    actor: &QueueActionActor,
) -> RawMaterialEventDraft {
    RawMaterialEventDraft {
        idempotency_key: format!(
            "{}:{}:{}:{}",
            event_type,
            assignment.barcode.trim().to_ascii_uppercase(),
            assignment.order_id.trim(),
            apparatus_id.trim()
        ),
        event_type: event_type.to_string(),
        warehouse: stock.warehouse.trim().to_string(),
        barcode: stock.barcode.trim().to_string(),
        item_code: stock.item_code.trim().to_string(),
        item_name: blank_default(&assignment.item_name, &stock.item_name).to_string(),
        qty_delta: 0.0,
        uom: stock.uom.trim().to_string(),
        stock_status_before: Some(stock.status.trim().to_string()),
        stock_status_after: Some(stock.status.trim().to_string()),
        order_id: Some(assignment.order_id.trim().to_string()),
        apparatus: Some(apparatus_id.trim().to_string()),
        actor_role: actor.role.trim().to_string(),
        actor_ref: actor.ref_.trim().to_string(),
        actor_display_name: actor.display_name.trim().to_string(),
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
            "apparatus_id": apparatus_id.trim(),
            "barcode": assignment.barcode.trim(),
            "item_group": assignment.item_group.trim(),
            "source_receipt_id": stock.source_receipt_id.trim(),
            "qty": stock.qty,
        }),
    }
}

fn assignment_transfer_event_draft(
    assignment: &RawMaterialAssignment,
    stock: &AssignmentStockRow,
    event_type: &str,
    apparatus_id: &str,
    transfer_id: &str,
    actor: &QueueActionActor,
) -> RawMaterialEventDraft {
    let mut draft = assignment_event_draft_for(
        assignment,
        stock,
        event_type,
        apparatus_id,
        actor,
    );
    let transfer_id = transfer_id.trim();
    if !transfer_id.is_empty() {
        draft.idempotency_key = format!(
            "apparatus_transfer:{}:{}",
            transfer_id, draft.idempotency_key
        );
        draft.correlation_id = Some(transfer_id.to_string());
    }
    draft
}

fn blank_default<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim()
    } else {
        value
    }
}
