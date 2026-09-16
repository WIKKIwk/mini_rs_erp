use super::*;
use crate::core::apparatus_standard::{
    ProcessTechnology,
    test_support::{TestApparatusSpec, runtime_configuration},
};
use crate::core::formula::LayerInput;
use crate::core::production_map::{
    automatic::{self, OrderProductionOptions, PrintMethod},
    queue_state::ApparatusQueueOrderState as State,
};
use std::collections::BTreeMap;

#[test]
fn order_edit_blocks_first_on_any_apparatus_and_first_actionable() {
    let mut sequences = BTreeMap::from([(
        "apparatus:test:a".into(),
        vec!["first".into(), "edit".into()],
    )]);
    let mut states = BTreeMap::new();
    assert!(check_queue_position("edit", &sequences, &states).is_ok());
    let reason = check_queue_position("first", &sequences, &states)
        .unwrap_err()
        .to_string();
    assert!(reason.contains("navbatida birinchi"));
    assert!(reason.contains("Barcha apparatlardagi navbatini tekshiring"));
    states.insert(
        "apparatus:test:a".into(),
        BTreeMap::from([("first".into(), State::Completed)]),
    );
    assert!(check_queue_position("edit", &sequences, &states).is_err());
    states.clear();
    sequences.insert(
        "apparatus:test:b".into(),
        vec!["edit".into(), "first".into()],
    );
    assert!(check_queue_position("edit", &sequences, &states).is_err());
}

#[test]
fn order_edit_route_accepts_kg_but_rejects_incompatible_dimensions_layers_and_splits() {
    let mut print = TestApparatusSpec::print(
        "apparatus:test:print",
        "Print",
        ProcessTechnology::Flexographic,
        Some(8),
    );
    print.max_web_width_mm = Some(1500);
    let mut lam = TestApparatusSpec::laminate("apparatus:test:lam", "Laminate");
    lam.max_web_width_mm = Some(1000);
    let catalog = vec![
        runtime_configuration(print),
        runtime_configuration(lam),
        runtime_configuration(TestApparatusSpec::cut("apparatus:test:cut", "Cut")),
    ];
    let original = CalculateOrderTemplate {
        name: "Order".into(),
        product: "Product".into(),
        item_code: "item".into(),
        kg: 500.0,
        frame_product_size_mm: 400.0,
        frame_count: 3.0,
        roll_count: Some(6),
        status: "rulon".into(),
        layers: vec![LayerInput::new("pet", "12"), LayerInput::new("pet", "12")],
        production_options: Some(OrderProductionOptions {
            print_method: PrintMethod::Flexo,
            cold_glue: false,
            diameter_mm: None,
        }),
        ..Default::default()
    };
    let map = automatic::generate(&original, &catalog, &[]).unwrap();
    let mut updated = original.clone();
    updated.kg = 600.0;
    assert!(validate_route(&original, &updated, &map, &catalog).is_ok());
    updated.roll_count = Some(9);
    let reason = validate_route(&original, &updated, &map, &catalog)
        .unwrap_err()
        .to_string();
    assert!(reason.contains("Print"));
    assert!(reason.contains("rang soni"));
    updated = original.clone();
    updated.frame_product_size_mm = 600.0;
    let reason = validate_route(&original, &updated, &map, &catalog)
        .unwrap_err()
        .to_string();
    assert!(reason.contains("en"));
    updated = original.clone();
    updated.layers.pop();
    assert!(
        validate_route(&original, &updated, &map, &catalog)
            .unwrap_err()
            .to_string()
            .contains("qatlam")
    );
    updated = original.clone();
    updated.status = "paket".into();
    assert!(
        validate_route(&original, &updated, &map, &catalog)
            .unwrap_err()
            .to_string()
            .contains("Buyurtma turi")
    );
    let mut split = map.clone();
    split
        .nodes
        .iter_mut()
        .find(|n| !n.rezka_frame_groups.is_empty())
        .unwrap()
        .rezka_frame_groups = vec![2, 2];
    assert!(validate_route(&original, &original, &split, &catalog).is_err());
}

#[test]
fn order_edit_generic_map_editor_cannot_change_calculation_fields() {
    let before: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
        "id": "zakaz-1234", "product_code": "P", "title": "P", "order_number": "1234", "order_kg": 500.0,
    })).unwrap();
    let mut after = before.clone();
    assert!(!calculation_map_changed(&before, &after));
    after.order_kg = Some(600.0);
    assert!(calculation_map_changed(&before, &after));
}
