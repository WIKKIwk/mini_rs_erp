use crate::core::calculate_materials::default_calculate_materials;
use crate::core::formula::{
    CalculateRequest, LayerInput, calculate, calculate_with_material_catalog,
};

#[test]
fn calculates_formula_with_waste_and_rounding() {
    let result = calculate(CalculateRequest {
        kg: Some(300.0),
        frame_product_size_mm: Some(515.0),
        frame_count: Some(1.0),
        first_layer: LayerInput::new("pet", "12"),
        second_layer: LayerInput::new("pe oq", "30"),
        ..CalculateRequest::default()
    })
    .expect("calculate");

    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].rounded_length, 13000.0);
    assert!((result.results[0].film_gsm - 44.4).abs() < 0.001);
    assert!((result.results[0].adhesive_gsm - 2.5).abs() < 0.001);
    assert!((result.results[0].total_gsm - 46.9).abs() < 0.001);
    assert!((result.results[0].base_length - 12069.0349).abs() < 0.001);
    assert!((result.results[0].waste_length - 635.2124).abs() < 0.001);
}

#[test]
fn calculates_single_layer_order() {
    let result = calculate(CalculateRequest {
        kg: Some(300.0),
        frame_product_size_mm: Some(515.0),
        frame_count: Some(1.0),
        first_layer: LayerInput::new("pet", "12"),
        ..CalculateRequest::default()
    })
    .expect("calculate single layer");

    assert_eq!(result.layers.len(), 1);
    assert_eq!(result.layers[0].material, "pet");
    assert_eq!(result.results[0].adhesive_gsm, 0.0);
    assert_eq!(result.results[0].other_coeff, 0.0);
    assert!(result.results[0].base_length > 0.0);
}

#[test]
fn calculates_order_with_arbitrary_layer_count() {
    let layers = [
        ("pet", "12"),
        ("pe oq", "30"),
        ("pe pr", "40"),
        ("mcp", "25"),
        ("jem", "20"),
        ("opp", "18"),
    ]
    .into_iter()
    .map(|(material, micron)| LayerInput::new(material, micron))
    .collect();
    let result = calculate(CalculateRequest {
        kg: Some(300.0),
        frame_product_size_mm: Some(515.0),
        frame_count: Some(1.0),
        layers,
        ..CalculateRequest::default()
    })
    .expect("calculate arbitrary layers");

    assert_eq!(result.layers.len(), 6);
    assert!(result.results[0].first_coeff > 0.0);
    assert!(result.results[0].other_coeff > 0.0);
    assert!(result.results[0].base_length > 0.0);
}

#[test]
fn calculates_with_custom_waste_percent() {
    let result = calculate(CalculateRequest {
        kg: Some(300.0),
        frame_product_size_mm: Some(515.0),
        frame_count: Some(1.0),
        waste_percent: Some(10.0),
        first_layer: LayerInput::new("pet", "12"),
        second_layer: LayerInput::new("pe oq", "30"),
        ..CalculateRequest::default()
    })
    .expect("calculate");

    assert_eq!(result.waste_percent, 10.0);
    assert_eq!(result.results[0].rounded_length, 13500.0);
    assert!((result.results[0].waste_length - 1341.0039).abs() < 0.001);
}

#[test]
fn calculates_rubber_size_from_width() {
    let cases = [(645.0, 650), (670.0, 700), (50.0, 50), (1400.0, 1300)];

    for (width_mm, rubber_size_mm) in cases {
        let result = calculate(CalculateRequest {
            kg: Some(300.0),
            frame_product_size_mm: Some(width_mm - 15.0),
            frame_count: Some(1.0),
            first_layer: LayerInput::new("pet", "12"),
            second_layer: LayerInput::new("pe oq", "30"),
            ..CalculateRequest::default()
        })
        .expect("calculate");

        assert_eq!(result.rubber_size_mm, rubber_size_mm);
    }
}

#[test]
fn calculates_min_mold_size_at_least_50mm_above_rubber_size() {
    let result = calculate(CalculateRequest {
        kg: Some(300.0),
        frame_product_size_mm: Some(250.0),
        frame_count: Some(3.0),
        first_layer: LayerInput::new("pet", "12"),
        second_layer: LayerInput::new("pe oq", "30"),
        ..CalculateRequest::default()
    })
    .expect("calculate");

    assert_eq!(result.width_mm, 765.0);
    assert_eq!(result.rubber_size_mm, 800);
    assert_eq!(result.min_mold_size_mm, 850.0);
}

#[test]
fn calculates_alternative_material_variants() {
    let result = calculate(CalculateRequest {
        kg: Some(300.0),
        frame_product_size_mm: Some(515.0),
        frame_count: Some(1.0),
        first_layer: LayerInput::new("pet", "12"),
        second_layer: LayerInput::new("pe oq yoki mcp", "30"),
        ..CalculateRequest::default()
    })
    .expect("calculate");

    let total_gsm = result
        .results
        .into_iter()
        .map(|result| result.total_gsm)
        .collect::<Vec<_>>();
    assert_eq!(total_gsm.len(), 2);
    assert!((total_gsm[0] - 46.9).abs() < 0.001);
    assert!((total_gsm[1] - 46.45).abs() < 0.001);
}

#[test]
fn alternate_microns_produce_distinct_lengths_and_keep_the_selected_plan_first() {
    let mut request = CalculateRequest {
        kg: Some(100.0),
        frame_product_size_mm: Some(985.0),
        frame_count: Some(1.0),
        layers: vec![LayerInput {
            alternative_microns: vec![18],
            ..LayerInput::with_material_id("builtin-opp", "OPP", "12")
        }],
        ..CalculateRequest::default()
    };
    let catalog = default_calculate_materials();
    let result = calculate_with_material_catalog(request.clone(), &catalog).unwrap();
    assert_eq!(result.results.len(), 2);
    assert_eq!(result.results[0].rounded_length, 10_000.0);
    assert!(result.results[0].micron_overrides.is_empty());
    assert_eq!(result.results[1].rounded_length, 6_500.0);
    assert_eq!(result.results[1].micron_overrides[0].layer_index, 0);
    assert_eq!(result.results[1].micron_overrides[0].micron, 18);

    request.layers[0].planned_micron = Some(18);
    let result = calculate_with_material_catalog(request.clone(), &catalog).unwrap();
    assert_eq!(result.results[0].rounded_length, 6_500.0);
    assert_eq!(result.results[1].rounded_length, 10_000.0);
    assert_eq!(result.layers[0].micron, "12");

    request.layers[0].planned_micron = Some(30);
    assert!(calculate_with_material_catalog(request, &catalog).is_err());
}

#[test]
fn alternatives_on_different_layers_keep_the_correct_layer_and_adhesive() {
    let request = CalculateRequest {
        kg: Some(100.0),
        frame_product_size_mm: Some(985.0),
        frame_count: Some(1.0),
        layers: vec![
            LayerInput { alternative_microns: vec![18], ..LayerInput::new("OPP", "12") },
            LayerInput { alternative_microns: vec![20], ..LayerInput::new("PET", "12") },
        ],
        ..CalculateRequest::default()
    };
    let results = calculate_with_material_catalog(request, &default_calculate_materials()).unwrap().results;
    assert_eq!(results.len(), 4);
    assert!(results.iter().all(|result| result.adhesive_gsm == 2.5));
    let combined = results.iter().find(|result| result.micron_overrides.len() == 2).unwrap();
    assert_eq!(combined.micron_overrides[0].layer_index, 0);
    assert_eq!(combined.micron_overrides[0].micron, 18);
    assert_eq!(combined.micron_overrides[1].layer_index, 1);
    assert_eq!(combined.micron_overrides[1].micron, 20);
    assert!((combined.total_gsm - (16.29 + 28.0 + 2.5)).abs() < 0.001);
}

#[test]
fn parses_material_display_when_layers_are_empty() {
    let result = calculate(CalculateRequest {
        kg: Some(3000.0),
        frame_product_size_mm: Some(620.0),
        frame_count: Some(1.0),
        material_display: Some("pet 12 + oppm/pe pr 20/30".to_string()),
        ..CalculateRequest::default()
    })
    .expect("calculate");

    assert_eq!(result.results[0].rounded_length, 77000.0);
    assert_eq!(result.layers[0].material, "pet");
    assert_eq!(result.layers[1].material, "oppm/pe pr");
}

#[test]
fn calculates_user_example_with_physical_gsm_and_yield_waste() {
    let result = calculate(CalculateRequest {
        kg: Some(1000.0),
        frame_product_size_mm: Some(250.0),
        frame_count: Some(3.0),
        waste_percent: Some(5.0),
        first_layer: LayerInput::new("PET", "12"),
        second_layer: LayerInput::new("PET", "12"),
        ..CalculateRequest::default()
    })
    .expect("calculate user example");

    let value = &result.results[0];
    assert_eq!(result.width_mm, 765.0);
    assert!((value.film_gsm - 33.6).abs() < 0.001);
    assert!((value.adhesive_gsm - 2.5).abs() < 0.001);
    assert!((value.total_gsm - 36.1).abs() < 0.001);
    assert!((value.base_length - 36210.2366).abs() < 0.001);
    assert!((value.waste_length - 1905.8019).abs() < 0.001);
    assert_eq!(value.rounded_length, 38500.0);
}

#[test]
fn rejects_waste_percent_that_cannot_be_a_yield() {
    let error = calculate(CalculateRequest {
        kg: Some(1000.0),
        frame_product_size_mm: Some(250.0),
        frame_count: Some(3.0),
        waste_percent: Some(100.0),
        first_layer: LayerInput::new("PET", "12"),
        ..CalculateRequest::default()
    })
    .expect_err("100% waste must be rejected");

    assert_eq!(error, "Atxod foiz noto'g'ri");
}

#[test]
fn catalog_resolves_pe_oq_as_a_separate_material() {
    let value = calculate_with_material_catalog(
        CalculateRequest {
            kg: Some(1000.0),
            frame_product_size_mm: Some(250.0),
            frame_count: Some(3.0),
            first_layer: LayerInput::new("PE oq", "30"),
            ..CalculateRequest::default()
        },
        &default_calculate_materials(),
    )
    .expect("PE oq must be available as a separate material");

    assert_eq!(value.results.len(), 1);
    assert_eq!(value.results[0].film_gsm, 27.6);
}

#[test]
fn catalog_density_calculates_an_unlisted_micron() {
    let value = calculate_with_material_catalog(
        CalculateRequest {
            kg: Some(1000.0),
            frame_product_size_mm: Some(250.0),
            frame_count: Some(3.0),
            first_layer: LayerInput::new("PET", "19"),
            ..CalculateRequest::default()
        },
        &default_calculate_materials(),
    )
    .expect("density should calculate an unlisted micron");

    assert_eq!(value.results.len(), 1);
    assert!((value.results[0].film_gsm - 26.6).abs() < 0.001);
}

#[test]
fn catalog_still_rejects_an_unknown_material_name() {
    let error = calculate_with_material_catalog(
        CalculateRequest {
            kg: Some(1000.0),
            frame_product_size_mm: Some(250.0),
            frame_count: Some(3.0),
            first_layer: LayerInput::new("PE white", "30"),
            ..CalculateRequest::default()
        },
        &default_calculate_materials(),
    )
    .expect_err("unknown material names must not behave as aliases");

    assert_eq!(error, "material katalogdan tanlanmagan: PE white");
}
