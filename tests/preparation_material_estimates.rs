use mini_rs_erp::core::{
    calculate_materials::{CalculateMaterial, CalculateMaterialVariant},
    formula::{LayerInput, planned_material_kg},
};

fn catalog() -> Vec<CalculateMaterial> {
    vec![
        CalculateMaterial {
            id: "opp".into(),
            name: "OPP".into(),
            active: true,
            sort_order: None,
            density_g_cm3: 0.905,
            variants: vec![],
        },
        CalculateMaterial {
            id: "pe-pr".into(),
            name: "PE PR".into(),
            active: true,
            sort_order: None,
            density_g_cm3: 0.92,
            variants: vec![],
        },
    ]
}

#[test]
fn selected_layer_uses_saved_planned_dimensions_without_glue_or_extra_waste() {
    let layers = vec![
        LayerInput::with_material_id("opp", "OPP", "18"),
        LayerInput::with_material_id("pe-pr", "PE PR", "70"),
    ];
    let kg = planned_material_kg(&layers, "pe-pr", 915.0, 14_000.0, &catalog()).unwrap();
    assert!((kg - 824.964).abs() < 0.000001);
    // The same shared recipe has a different mass for another order.
    let other = planned_material_kg(&layers, "pe-pr", 835.0, 16_000.0, &catalog()).unwrap();
    assert!((other - 860.384).abs() < 0.000001);
}

#[test]
fn repeated_material_layers_are_summed_and_actual_gsm_overrides_density() {
    let mut materials = catalog();
    materials[1].variants.push(CalculateMaterialVariant {
        micron: 70,
        coefficient: 0.0,
        first_layer_coefficient: None,
        actual_gsm: Some(60.0),
    });
    let layers = vec![
        LayerInput::with_material_id("pe-pr", "Renamed PE", "70"),
        LayerInput::with_material_id("opp", "OPP", "18"),
        LayerInput::with_material_id("PE-PR", "PE PR", "30"),
    ];
    let kg = planned_material_kg(&layers, "PE-PR", 1000.0, 1000.0, &materials).unwrap();
    assert!((kg - 87.6).abs() < 0.000001);
}

#[test]
fn legacy_layer_name_resolves_to_catalog_material() {
    let kg = planned_material_kg(
        &[LayerInput::new("pe pr", "70")],
        "pe-pr",
        915.0,
        14_000.0,
        &catalog(),
    )
    .unwrap();
    assert!((kg - 824.964).abs() < 0.000001);
}

#[test]
fn incomplete_or_ambiguous_inputs_never_fall_back_to_entire_order_mass() {
    let materials = catalog();
    for micron in ["", "0", "70/30", "abc"] {
        let layers = [LayerInput::with_material_id("pe-pr", "PE PR", micron)];
        assert!(planned_material_kg(&layers, "pe-pr", 915.0, 14_000.0, &materials).is_err());
    }
    let layers = [LayerInput::with_material_id("pe-pr", "PE PR", "70")];
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(planned_material_kg(&layers, "pe-pr", invalid, 14_000.0, &materials).is_err());
        assert!(planned_material_kg(&layers, "pe-pr", 915.0, invalid, &materials).is_err());
    }
    assert!(planned_material_kg(&layers, "missing", 915.0, 14_000.0, &materials).is_err());
    assert!(planned_material_kg(&[], "pe-pr", 915.0, 14_000.0, &materials).is_err());
    let mut missing_density = materials;
    missing_density[1].density_g_cm3 = 0.0;
    assert!(planned_material_kg(&layers, "pe-pr", 915.0, 14_000.0, &missing_density).is_err());
}
