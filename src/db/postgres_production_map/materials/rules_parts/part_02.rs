
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_assignment_requires_available_unreserved_stock() {
        assert_eq!(
            ensure_assignment_stock_available("available", ""),
            Ok(())
        );
        assert_eq!(
            ensure_assignment_stock_available("in_use", ""),
            Err(ProductionMapError::RawMaterialStockUnavailable)
        );
        assert_eq!(
            ensure_assignment_stock_available("available", "order-001"),
            Err(ProductionMapError::RawMaterialStockUnavailable)
        );
    }

    #[test]
    fn consumed_stock_cannot_follow_an_apparatus_transfer() {
        let stock = AssignmentStockRow {
            warehouse: "Raw warehouse".to_string(),
            item_code: "FILM-001".to_string(),
            item_name: "Film".to_string(),
            barcode: "RM-001".to_string(),
            qty: 4.5,
            uom: "kg".to_string(),
            status: "consumed".to_string(),
            reserved_order_id: "ORDER-001".to_string(),
            source_receipt_id: "receipt-001".to_string(),
        };

        assert_eq!(
            ensure_assignment_transferable(&stock, "ORDER-001"),
            Err(ProductionMapError::RawMaterialAssignmentLocked)
        );
    }

    #[test]
    fn apparatus_transfer_events_keep_old_and_new_canonical_ids() {
        let assignment = raw_material_assignment_from_payload(
            "apparatus:catalog:new-stage".to_string(),
            serde_json::json!({
                "order_id": "ORDER-001",
                "apparatus": "New stage",
                "barcode": "RM-001",
                "item_code": "FILM-001",
                "item_name": "Film",
                "item_group": "Rulon",
                "assigned_by_role": "material_taminotchi",
                "assigned_by_ref": "worker-001",
                "assigned_by_display_name": "Worker",
                "assigned_at": "2026-08-19T00:00:00Z"
            }),
        )
        .expect("assignment payload");
        let stock = AssignmentStockRow {
            warehouse: "Raw warehouse".to_string(),
            item_code: "FILM-001".to_string(),
            item_name: "Film".to_string(),
            barcode: "RM-001".to_string(),
            qty: 4.5,
            uom: "kg".to_string(),
            status: "in_use".to_string(),
            reserved_order_id: "ORDER-001".to_string(),
            source_receipt_id: "receipt-001".to_string(),
        };
        let actor = QueueActionActor {
            role: "admin".to_string(),
            ref_: "admin-001".to_string(),
            display_name: "Admin".to_string(),
        };

        let unreserved = assignment_transfer_event_draft(
            &assignment,
            &stock,
            "order_unreserved",
            "apparatus:catalog:old-stage",
            "apparatus-transfer:001",
            &actor,
        );
        let reserved = assignment_transfer_event_draft(
            &assignment,
            &stock,
            "order_reserved",
            assignment.apparatus_id.as_str(),
            "apparatus-transfer:001",
            &actor,
        );

        assert_eq!(
            unreserved.apparatus.as_deref(),
            Some("apparatus:catalog:old-stage")
        );
        assert_eq!(
            unreserved.payload_json["apparatus_id"],
            serde_json::json!("apparatus:catalog:old-stage")
        );
        assert_eq!(
            reserved.apparatus.as_deref(),
            Some("apparatus:catalog:new-stage")
        );
        assert_ne!(unreserved.idempotency_key, reserved.idempotency_key);
        assert_eq!(
            reserved.correlation_id.as_deref(),
            Some("apparatus-transfer:001")
        );
    }

    #[test]
    fn loaded_assignment_backfills_canonical_id_without_replacing_display_snapshot() {
        let assignment = raw_material_assignment_from_payload(
            "apparatus:catalog:lam-001".to_string(),
            serde_json::json!({
                "order_id": "zakaz-001",
                "apparatus_id": "Laminatsiya (legacy)",
                "apparatus": "Laminatsiya (legacy)",
                "barcode": "RM-001",
                "item_code": "FILM-001",
                "item_name": "Film",
                "item_group": "Rulon",
                "assigned_by_role": "material_taminotchi",
                "assigned_by_ref": "worker-001",
                "assigned_by_display_name": "Worker",
                "assigned_at": "2026-08-19T00:00:00Z"
            }),
        )
        .expect("legacy payload should load with the storage identity");

        assert_eq!(
            assignment.apparatus_id.as_str(),
            "apparatus:catalog:lam-001"
        );
        assert_eq!(assignment.apparatus, "Laminatsiya (legacy)");
    }
}
