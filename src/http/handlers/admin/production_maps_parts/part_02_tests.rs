use super::*;

fn assignment() -> RawMaterialAssignment {
    RawMaterialAssignment {
        order_id: "zakaz-001".to_string(),
        apparatus_id: ApparatusId::new("apparatus:catalog:lam-001".to_string())
            .expect("valid apparatus id"),
        apparatus: "Laminatsiya (historical title)".to_string(),
        barcode: "RM-001".to_string(),
        item_code: "FILM-001".to_string(),
        item_name: "Film".to_string(),
        item_group: "Rulon".to_string(),
        assigned_by_role: "material_taminotchi".to_string(),
        assigned_by_ref: "worker-001".to_string(),
        assigned_by_display_name: "Worker".to_string(),
        assigned_at: "2026-08-19T00:00:00Z".to_string(),
    }
}

#[test]
fn renamed_historical_display_name_still_matches_by_canonical_id() {
    assert!(raw_material_assignment_matches_order_apparatus(
        &assignment(),
        "zakaz-001",
        "apparatus:catalog:lam-001",
    ));
}

#[test]
fn title_only_apparatus_mismatch_cannot_select_completion_material() {
    let assignment = assignment();

    assert!(!raw_material_assignment_matches_order_apparatus(
        &assignment,
        "zakaz-001",
        "Laminatsiya (historical title)",
    ));
    assert!(!raw_material_assignment_matches_order_apparatus(
        &assignment,
        "zakaz-001",
        "Laminatsiya",
    ));
}
