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
        RawMaterialStockTransitionKind::Consumed | RawMaterialStockTransitionKind::Complete => {
            ("consumption_posted", "consumed", -row.qty)
        }
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
            "apparatus_id": apparatus.trim(),
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

fn ensure_stock_transition_rows_affected(
    kind: RawMaterialStockTransitionKind,
    requested_rows: usize,
    affected_rows: usize,
) -> Result<(), ProductionMapError> {
    if matches!(
        kind,
        RawMaterialStockTransitionKind::InUse | RawMaterialStockTransitionKind::Consumed
    ) && requested_rows != affected_rows
    {
        return Err(ProductionMapError::RawMaterialStockUnavailable);
    }
    Ok(())
}
