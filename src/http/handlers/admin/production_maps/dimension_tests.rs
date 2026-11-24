use super::*;

#[test]
fn structured_stock_width_wins_over_legacy_item_text() {
    let stock = RawMaterialStockEntry {
        item_code: "pet".to_string(),
        item_name: "PET 615/13".to_string(),
        width_mm: Some(620.0),
        micron: Some(13.0),
        ..RawMaterialStockEntry::default()
    };
    let item = SupplierItem {
        code: "PET 615/12".to_string(),
        name: "PET 615/12".to_string(),
        ..SupplierItem::default()
    };

    assert_eq!(roll_width_mm(&stock, &item), Some(620.0));
}

#[test]
fn legacy_stock_still_reads_width_from_text() {
    let stock = RawMaterialStockEntry {
        item_name: "PET 615/13".to_string(),
        ..RawMaterialStockEntry::default()
    };

    assert_eq!(roll_width_mm(&stock, &SupplierItem::default()), Some(615.0));
}

#[test]
fn tayyorlov_masteri_bypasses_only_the_rulon_width_restriction() {
    assert!(!rulon_width_restriction_applies(
        PrincipalRole::TayyorlovMasteri
    ));
    assert!(rulon_width_restriction_applies(
        PrincipalRole::MaterialTaminotchi
    ));
    assert!(rulon_width_restriction_applies(PrincipalRole::Admin));
}
