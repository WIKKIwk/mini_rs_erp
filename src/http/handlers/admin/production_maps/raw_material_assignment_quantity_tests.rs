use super::*;

#[test]
fn preparation_roll_is_attached_but_only_cut_children_are_executable() {
    assert_eq!(roll_execution_status(Some(1015.0), Some(2030.0), 20.0), "needs_cutting");
    assert_eq!(roll_execution_status(Some(1015.0), Some(1015.0), 20.0), "compatible");
    assert_eq!(roll_execution_status(Some(1015.0), Some(1035.0), 20.0), "compatible");
    assert_eq!(roll_execution_status(Some(1015.0), Some(1045.0), 20.0), "compatible");
    assert_eq!(roll_execution_status(Some(1015.0), Some(1046.0), 20.0), "needs_cutting");
    assert_eq!(roll_execution_status(Some(1015.0), Some(1005.0), 20.0), "compatible");
    assert_eq!(roll_execution_status(Some(1015.0), Some(1004.0), 20.0), "width_mismatch");
    assert_eq!(roll_execution_status(None, Some(1015.0), 20.0), "dimensions_missing");
}

fn assignment() -> RawMaterialAssignment {
    RawMaterialAssignment {
        order_id: "zakaz-1".to_string(),
        apparatus_id: crate::core::apparatus_standard::ApparatusId::new(
            "apparatus:test:pechat-1",
        )
        .unwrap(),
        apparatus: "Pechat".to_string(),
        barcode: "ROLL-1000".to_string(),
        item_code: "RULON".to_string(),
        item_name: "Rulon".to_string(),
        item_group: "Rulon".to_string(),
        assigned_by_role: "aparatchi".to_string(),
        assigned_by_ref: "worker-1".to_string(),
        assigned_by_display_name: "Worker".to_string(),
        assigned_at: String::new(),
    }
}

fn stock(status: &str, order_id: &str) -> RawMaterialStockEntry {
    RawMaterialStockEntry {
        qty: 1_000.0,
        status: status.to_string(),
        reserved_order_id: order_id.to_string(),
        ..RawMaterialStockEntry::default()
    }
}

#[test]
fn quantity_ledger_preserves_received_consumed_remaining_invariant() {
    let assignment = assignment();
    assert_eq!(
        raw_material_assignment_quantities(&assignment, Some(&stock("available", ""))),
        (0.0, 0.0, 0.0)
    );
    assert_eq!(
        raw_material_assignment_quantities(&assignment, Some(&stock("in_use", "zakaz-1"))),
        (1_000.0, 0.0, 1_000.0)
    );
    assert_eq!(
        raw_material_assignment_quantities(&assignment, Some(&stock("consumed", "zakaz-1"))),
        (1_000.0, 1_000.0, 0.0)
    );
    assert_eq!(
        raw_material_assignment_quantities(&assignment, Some(&stock("in_use", "zakaz-2"))),
        (0.0, 0.0, 0.0)
    );
}

#[test]
fn staged_placement_matches_canonical_id_not_display_name() {
    let placement = RawMaterialStatePlacement {
        barcode: "ROLL-1000".to_string(),
        location_id: "location:state:pechat".to_string(),
        location_name: "Pechat oldi".to_string(),
        apparatus_ids: vec!["apparatus:catalog:pechat-001".to_string()],
        apparatus: vec!["7 ta rangli pechat - A".to_string()],
    };

    assert!(state_placement_matches_apparatus(
        &placement,
        "apparatus:catalog:pechat-001"
    ));
    assert!(!state_placement_matches_apparatus(
        &placement,
        "7 ta rangli pechat - A"
    ));
}
