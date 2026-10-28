use super::*;
use crate::core::formula::LayerInput;
use crate::core::order_layout::OrderProduct;

pub(crate) fn template() -> CalculateOrderTemplate {
    crate::core::calculate_orders::hydrate_template_dimensions(CalculateOrderTemplate {
        products: vec![
            OrderProduct {
                id: "a".into(),
                item_code: "SKU-A".into(),
                product: "A".into(),
                frame_product_size_mm: 115.0,
                frame_count: 1,
            },
            OrderProduct {
                id: "b".into(),
                item_code: "SKU-B".into(),
                product: "B".into(),
                frame_product_size_mm: 80.0,
                frame_count: 2,
            },
        ],
        customer_ref: "CUSTOMER".into(),
        customer: "Customer".into(),
        layers: vec![LayerInput::new("pet", "12")],
        edge_allowance_mm: 15.0,
        status: "Rulon".into(),
        kg: 300.0,
        waste_percent: 5.0,
        ..Default::default()
    })
}

fn reference(template: &CalculateOrderTemplate) -> ComboReference {
    ComboReference {
        id: "combo-a".into(),
        item_code: "COMBO-SKU".into(),
        name: "Morning combo".into(),
        revision: 1,
        layout_fingerprint: layout_fingerprint(template).unwrap(),
    }
}

#[test]
fn layout_identity_ignores_quantities_notes_and_transient_row_ids() {
    let original = template();
    let mut changed = original.clone();
    changed.kg = 900.0;
    changed.waste_percent = 8.0;
    changed.note = "Another run".into();
    changed.products[0].id = "new-row-id".into();
    changed.products[0].product = "Renamed display label".into();
    assert_eq!(
        layout_fingerprint(&original).unwrap(),
        layout_fingerprint(&changed).unwrap()
    );
}

#[test]
fn layout_identity_tracks_order_dimensions_customer_recipe_and_artwork() {
    let original = template();
    let expected = layout_fingerprint(&original).unwrap();
    let mut variants = Vec::new();
    let mut changed = original.clone();
    changed.products.reverse();
    variants.push(changed);
    let mut changed = original.clone();
    changed.products[0].frame_product_size_mm += 1.0;
    variants.push(changed);
    let mut changed = original.clone();
    changed.products[1].frame_count += 1;
    variants.push(changed);
    let mut changed = original.clone();
    changed.customer_ref = "OTHER".into();
    variants.push(changed);
    let mut changed = original.clone();
    changed.layers[0].micron = "20".into();
    variants.push(changed);
    let mut changed = original.clone();
    changed.image_id = "artwork-v2".into();
    variants.push(changed);
    let mut changed = original.clone();
    changed.print_val_size_mm = Some(600.0);
    variants.push(changed);
    for variant in variants {
        assert_ne!(expected, layout_fingerprint(&variant).unwrap());
    }
}

#[test]
fn named_parent_preserves_component_lanes_and_rejects_forged_primary_fields() {
    let ordinary = template();
    let combo = reference(&ordinary);
    let mut named = ordinary.clone();
    named.combo = Some(combo.clone());
    let named = crate::core::calculate_orders::hydrate_template_dimensions(named);
    assert_eq!(named.name, combo.name);
    assert_eq!(named.product, combo.name);
    assert_eq!(named.item_code, combo.item_code);
    assert_eq!(named.products, ordinary.products);
    validate_reference(&named).unwrap();
    let lanes = crate::core::order_layout::lanes(&named.products, "order", "1001");
    assert_eq!(
        lanes
            .iter()
            .map(|lane| lane.item_code.as_str())
            .collect::<Vec<_>>(),
        ["SKU-A", "SKU-B", "SKU-B"]
    );
    let mut forged = named.clone();
    forged.item_code = "OTHER".into();
    assert_eq!(
        validate_reference(&forged),
        Err(ComboError::ReferenceMismatch)
    );
    let mut empty = named;
    empty.products.clear();
    assert!(validate_reference(&empty).is_err());
}
