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
